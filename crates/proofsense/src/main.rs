use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "proofsense", about = "Proof-linting against reviewable literature")]
struct Cli { #[command(subcommand)] cmd: Cmd }

#[derive(Subcommand)]
enum Cmd { /// Check warrants in a manifest
    Check { manifest: std::path::PathBuf } }

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd { Cmd::Check { manifest } => { println!("checking {}", manifest.display()); Ok(()) } }
}
