//! statica CLI — reads `statica.toml`, resolves the project from cwd, then asks
//! core to parse → emit.
//!
//! Subcommands: default/`build`, `serve`, `watch`, `new`.
//! Man pages are generated into `docs/man/` by `build.rs` from the clap definitions.

mod cli;
mod cli_config;
mod cmd;
mod config;
mod style;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!(
            "{} {}",
            style::bold_stdout("statica"),
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }

    match cli.command {
        None => cmd::build::run(&cli.path, &cli.config),
        Some(Commands::Build(args)) => cmd::build::run(&args.path, &cli.config),
        Some(Commands::Serve(args)) => cmd::serve::run(&args.path, &cli.config).await,
        Some(Commands::Watch(args)) => cmd::watch::run(&args.path, &cli.config).await,
        Some(Commands::New(args)) => cmd::new::run(&args.name),
    }
}
