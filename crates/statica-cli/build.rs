//! Generate man pages from the clap Command definition into `docs/man/`.

use std::env;
use std::fs;
use std::path::PathBuf;

use clap::CommandFactory;

#[path = "src/cli_config.rs"]
mod cli_config;

#[path = "src/cli.rs"]
mod cli;

fn main() -> std::io::Result<()> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = manifest_dir.join("../../docs/man");
    fs::create_dir_all(&out_dir)?;

    let cmd = cli::Cli::command();
    clap_mangen::generate_to(cmd, &out_dir)?;

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=src/cli_config.rs");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
