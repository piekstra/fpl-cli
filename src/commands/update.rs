//! `fpl self-update` (alias `update`) — self-update from GitHub Releases via
//! the family updater (`pk-cli-selfupdate`). Release assets are named
//! `fpl-<target-triple>.tar.gz`; the triple is baked in by `build.rs`.

use pk_cli_selfupdate::{SelfUpdateArgs, Updater};

use crate::commands::Ctx;
use crate::error::AppError;

pub fn run(ctx: &Ctx, args: &SelfUpdateArgs) -> Result<(), AppError> {
    Updater {
        repo: "piekstra/fpl-cli".into(),
        binary: "fpl".into(),
        target: env!("FPL_TARGET").into(),
        current: env!("CARGO_PKG_VERSION").into(),
    }
    .run(args, ctx.cli.json, ctx.cli.quiet)
}
