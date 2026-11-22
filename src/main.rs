use std::process::ExitCode;

use clap::Parser;
use superbash::cli::{Cli, Mode};
use superbash::modes;
use superbash::npm::Npm;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let npm = Npm::default();
    let allow = cli.allow();
    let result = match cli.mode() {
        Mode::Mcp => modes::mcp(&npm, &cli.toolset, &allow, &cli.host, cli.port, cli.sandbox),
        Mode::Install => modes::install(&npm, &cli.toolset, &allow),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("superbash: {e:#}");
            ExitCode::FAILURE
        }
    }
}
