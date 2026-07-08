use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "proofsense",
    about = "Proof-linting against reviewable literature"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check warrants in a manifest
    Check {
        /// Path to the manifest JSON (subject_imports, sources, warrants).
        manifest: PathBuf,

        /// Use the deterministic StubEntailment test double instead of the
        /// LLM judge. Without this flag, entailment uses LlmEntailment,
        /// which requires PROOFSENSE_ENABLE_LLM and ANTHROPIC_API_KEY.
        #[arg(long)]
        stub: bool,

        /// Bypass spawning the Lean exe: parse a captured LeanDeclInfo JSON
        /// file (as emitted by `lake exe proofsense-lean`) instead, for
        /// every warrant in the manifest.
        #[arg(long, value_name = "FILE")]
        lean_info: Option<PathBuf>,

        /// Directory of the proofsense-lean Lake project, used to spawn
        /// `lake exe proofsense-lean` when --lean-info is not given.
        #[arg(long, default_value = "lean")]
        lean_dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Check {
            manifest,
            stub,
            lean_info,
            lean_dir,
        } => {
            let verdicts = proofsense::run_check(&manifest, lean_info.as_deref(), &lean_dir, stub)?;
            for v in &verdicts {
                println!("{}", proofsense::verdict::render_report(v));
                println!("{}", serde_json::to_string_pretty(v)?);
            }
            Ok(())
        }
    }
}
