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

    /// Resolve every warrant's locator and report what it hit, without
    /// spawning Lean or calling a judge. A warrant pointing at the wrong
    /// passage yields a verdict about the wrong theorem, so this gates a
    /// manifest before any expensive step runs.
    Resolve {
        /// Path to the manifest JSON.
        manifest: PathBuf,
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
            let report = proofsense::run_check(&manifest, lean_info.as_deref(), &lean_dir, stub)?;
            println!("{}", proofsense::report::render_header(&report));
            for v in &report.verdicts {
                println!("{}", proofsense::verdict::render_report(v));
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Cmd::Resolve { manifest } => {
            let resolutions = proofsense::resolve_manifest(&manifest)?;
            let unresolved = resolutions.iter().filter(|r| r.resolved.is_none()).count();

            for r in &resolutions {
                match &r.resolved {
                    Some(hit) => println!(
                        "  ok    {decl}\n        {locator} -> {hit}  ({chars} chars)\n        {head}",
                        decl = r.decl,
                        locator = r.locator,
                        chars = r.chars,
                        head = r.head.chars().take(96).collect::<String>(),
                    ),
                    None => println!(
                        "  MISS  {decl}\n        {locator} matched no passage in {source}",
                        decl = r.decl,
                        locator = r.locator,
                        source = r.source_id,
                    ),
                }
            }

            println!(
                "\n{} warrants, {} resolved, {} unresolved",
                resolutions.len(),
                resolutions.len() - unresolved,
                unresolved,
            );
            if unresolved > 0 {
                anyhow::bail!("{unresolved} warrant(s) resolved to no passage");
            }
            Ok(())
        }
    }
}
