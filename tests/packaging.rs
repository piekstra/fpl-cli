//! The Claude Desktop extension manifest (`packaging/mcpb/manifest.json`)
//! against the binary it packages, so a release cannot ship a manifest that
//! names a tool or a version the binary does not have. The release workflow
//! validates the schema with the `mcpb` CLI.

use std::process::Command;

use serde_json::{json, Value};

fn manifest() -> Value {
    let path = format!(
        "{}/packaging/mcpb/manifest.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_manifest_version_is_the_crate_version() {
    assert_eq!(manifest()["version"], env!("CARGO_PKG_VERSION"));
}

/// AGENTS.md names the spec's tag; a pin bump that forgets it leaves agents
/// reading the wrong spec.
#[test]
fn agents_md_names_the_pinned_cli_common_tag() {
    let root = env!("CARGO_MANIFEST_DIR");
    let cargo = std::fs::read_to_string(format!("{root}/Cargo.toml")).unwrap();
    let tag = cargo
        .lines()
        .find(|l| l.starts_with("pk-cli-core"))
        .and_then(|l| l.split("tag = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("pk-cli-core is pinned to a tag");
    let agents = std::fs::read_to_string(format!("{root}/AGENTS.md")).unwrap();
    assert!(
        agents.contains(&format!("**{tag}**")),
        "AGENTS.md does not name {tag}"
    );
}

#[test]
fn the_manifest_lists_exactly_the_served_tools() {
    let listed: Vec<String> = manifest()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    let out = Command::new(env!("CARGO_BIN_EXE_fpl"))
        .args(["mcp", "--list-tools"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let served: Vec<String> = serde_json::from_slice::<Value>(&out.stdout).unwrap()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed, served);
}

/// The settings form's values reach the binary through the variables it reads,
/// on every platform, and the password is stored as a secret.
#[test]
fn every_platform_passes_the_settings_as_the_variables_the_binary_reads() {
    let m = manifest();
    let config = &m["server"]["mcp_config"];
    let mut configs = vec![config.clone()];
    for os in ["win32", "linux"] {
        configs.push(config["platform_overrides"][os].clone());
    }
    for c in &configs {
        assert_eq!(c["args"], json!(["mcp"]), "{c}");
        assert!(
            c["command"]
                .as_str()
                .unwrap()
                .starts_with("${__dirname}/server/"),
            "{c}"
        );
        assert_eq!(
            c["env"],
            json!({
                "FPL_USERNAME": "${user_config.username}",
                "FPL_PASSWORD": "${user_config.password}"
            }),
            "{c}"
        );
    }
    assert_eq!(m["user_config"]["password"]["sensitive"], true);
    assert!(m["user_config"]["username"].get("sensitive").is_none());
}
