//! `fpl update` — self-update from GitHub Releases. `--check` reports whether a
//! newer version exists without installing; otherwise the running binary is
//! downloaded for this platform and replaced in place. Needs no FPL login.
//!
//! Release assets are named `fpl-<target-triple>.tar.gz` (see the release
//! workflow); [`FPL_TARGET`] is baked in at build time by `build.rs`.

use std::io::Read;
use std::time::Duration;

use serde_json::{json, Value};

use crate::cli::UpdateArgs;
use crate::commands::Ctx;
use crate::error::AppError;
use crate::output;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/piekstra/fpl/releases/latest";
const TARGET: &str = env!("FPL_TARGET");
const UA: &str = concat!("fpl-cli/", env!("CARGO_PKG_VERSION"));

pub fn run(ctx: &Ctx, args: &UpdateArgs) -> Result<(), AppError> {
    let current = env!("CARGO_PKG_VERSION");
    let release = latest_release()?;
    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let latest = tag.trim_start_matches('v');
    let available = version_gt(latest, current);

    if args.check {
        if args.json {
            output::json(&json!({
                "current": current,
                "latest": latest,
                "update_available": available,
            }));
        } else if available {
            println!("update available: {current} -> {latest} (run `fpl update`)");
        } else {
            println!("up to date ({current})");
        }
        return Ok(());
    }

    if !available {
        if !ctx.cli.quiet {
            eprintln!("already up to date ({current})");
        }
        if args.json {
            output::json(&json!({ "updated": false, "version": current }));
        }
        return Ok(());
    }

    let asset_url = asset_download_url(&release).ok_or_else(|| {
        AppError::NotFound(format!("release {tag} has no `fpl-{TARGET}.tar.gz` asset"))
    })?;

    if !ctx.cli.quiet {
        eprintln!("downloading {latest} for {TARGET}…");
    }
    let archive = download(&asset_url)?;
    let binary = extract_binary(&archive)?;
    replace_self(&binary)?;

    if !ctx.cli.quiet {
        eprintln!("updated to {latest}");
    }
    if args.json {
        output::json(&json!({ "updated": true, "version": latest }));
    }
    Ok(())
}

fn http() -> Result<reqwest::blocking::Client, AppError> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| AppError::Other(format!("failed to build HTTP client: {e}")))
}

fn latest_release() -> Result<Value, AppError> {
    let resp = http()?
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| AppError::Network(e.to_string()))?;
    if resp.status().as_u16() == 404 {
        return Err(AppError::NotFound(
            "no published releases for piekstra/fpl yet — build from source with `make install`"
                .into(),
        ));
    }
    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "GitHub API HTTP {} checking for releases",
            resp.status().as_u16()
        )));
    }
    resp.json::<Value>()
        .map_err(|e| AppError::Other(format!("parsing GitHub release JSON: {e}")))
}

fn asset_download_url(release: &Value) -> Option<String> {
    release
        .get("assets")
        .and_then(|a| a.as_array())?
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.contains(TARGET) && n.ends_with(".tar.gz"))
                .unwrap_or(false)
        })
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|u| u.as_str())
        .map(String::from)
}

fn download(url: &str) -> Result<Vec<u8>, AppError> {
    let resp = http()?
        .get(url)
        .send()
        .map_err(|e| AppError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "download failed: HTTP {}",
            resp.status().as_u16()
        )));
    }
    Ok(resp
        .bytes()
        .map_err(|e| AppError::Network(e.to_string()))?
        .to_vec())
}

/// Pull the `fpl` binary out of a `.tar.gz` archive.
fn extract_binary(archive: &[u8]) -> Result<Vec<u8>, AppError> {
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|e| AppError::Other(format!("reading update archive: {e}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| AppError::Other(format!("reading archive entry: {e}")))?;
        let is_bin = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n == "fpl"))
            .unwrap_or(false);
        if is_bin {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| AppError::Other(format!("extracting binary: {e}")))?;
            return Ok(buf);
        }
    }
    Err(AppError::NotFound(
        "the release archive did not contain an `fpl` binary".into(),
    ))
}

/// Write the new binary next to the current one and atomically swap it in.
fn replace_self(binary: &[u8]) -> Result<(), AppError> {
    let exe = std::env::current_exe()
        .map_err(|e| AppError::Other(format!("locating current executable: {e}")))?;
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp = dir.join(".fpl-update.tmp");
    std::fs::write(&tmp, binary)
        .map_err(|e| AppError::Other(format!("writing new binary to {}: {e}", tmp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| AppError::Other(format!("setting permissions: {e}")))?;
    }
    let result = self_replace::self_replace(&tmp)
        .map_err(|e| AppError::Other(format!("replacing the running binary: {e}")));
    let _ = std::fs::remove_file(&tmp);
    result
}

fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|x| x.trim().parse().unwrap_or(0))
            .collect()
    };
    let (pa, pb) = (parse(a), parse(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (
            pa.get(i).copied().unwrap_or(0),
            pb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_gt("0.2.0", "0.1.0"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(version_gt("0.1.1", "0.1.0"));
        assert!(!version_gt("0.1.0", "0.1.0"));
        assert!(!version_gt("0.1.0", "0.2.0"));
    }
}
