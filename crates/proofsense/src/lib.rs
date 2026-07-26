//! proofsense: proof-linting against reviewable literature.
//!
//! This library exposes the orchestrator's building blocks so both the
//! `proofsense` binary and the integration tests can use them.

pub mod entail;
pub mod hash;
pub mod ingest;
pub mod lean;
pub mod locator;
pub mod manifest;
pub mod verdict;

use anyhow::Context;
use entail::{Entailment, LlmEntailment, StubEntailment};
use manifest::Manifest;
use std::path::Path;
use verdict::{Judgement, TrustRung, Verdict};

/// Run the full pipeline (ingest -> resolve -> Lean bridge -> entailment ->
/// verdict) for every warrant in `manifest_path`, returning one [`Verdict`]
/// per warrant in manifest order.
///
/// - `lean_info_override`: when `Some(path)`, every warrant's
///   [`lean::LeanDeclInfo`] is parsed from that single captured JSON file
///   (via [`lean::parse_decl_info`]) instead of spawning the Lean exe. When
///   `None`, [`lean::extract_decl`] is invoked in `lean_dir`, per warrant,
///   using the manifest's `subject_imports`.
/// - `stub`: when `true`, entailment uses the deterministic
///   [`StubEntailment`] test double; when `false`, it uses
///   [`LlmEntailment::from_env`] (a real network call, itself gated behind
///   `PROOFSENSE_ENABLE_LLM` so it never fires unless explicitly enabled).
///
/// Trust rung per warrant: a locator that fails to resolve to any passage
/// yields [`TrustRung::Bare`] (a citation only, see the labelling
/// rule in `verdict.rs`); a resolved passage with an [`Judgement::Entailed`]
/// verdict yields [`TrustRung::Entailed`]; a resolved passage with any other
/// judgement yields [`TrustRung::Targeted`] (a concrete target, unconfirmed).
pub fn run_check(
    manifest_path: &Path,
    lean_info_override: Option<&Path>,
    lean_dir: &Path,
    stub: bool,
) -> anyhow::Result<Vec<Verdict>> {
    let raw = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .with_context(|| format!("parsing manifest JSON {}", manifest_path.display()))?;

    // Paths inside the manifest (e.g. each source's `content_list`) are
    // resolved relative to the manifest file's own directory, not the
    // process's current directory. This keeps a manifest portable: it works
    // identically whether invoked from the crate root (as `cargo test`
    // does) or from the workspace root (as `cargo run` does).
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    let entailment: Box<dyn Entailment> = if stub {
        Box::new(StubEntailment::default())
    } else {
        Box::new(LlmEntailment::from_env()?)
    };

    let mut verdicts = Vec::with_capacity(manifest.warrants.len());
    for warrant in &manifest.warrants {
        let source = manifest
            .sources
            .iter()
            .find(|s| s.id == warrant.source_id)
            .with_context(|| {
                format!(
                    "warrant for {:?} cites unknown source_id {:?}",
                    warrant.decl, warrant.source_id
                )
            })?;

        let content_list_path = if source.content_list.is_absolute() {
            source.content_list.clone()
        } else {
            manifest_dir.join(&source.content_list)
        };
        let passages = ingest::load_passages(&content_list_path)?;
        let resolved = locator::resolve(&passages, &warrant.locator);

        let lean_info = match lean_info_override {
            Some(path) => {
                let s = std::fs::read_to_string(path)
                    .with_context(|| format!("reading lean-info override {}", path.display()))?;
                lean::parse_decl_info(s.trim())?
            }
            None => lean::extract_decl(lean_dir, &warrant.decl, &manifest.subject_imports)?,
        };

        let (judgement, rationale, confidence, target_passage) = match resolved {
            Some(passage) => {
                let (j, r, c) = entailment.check(&passage.text, &lean_info.type_english)?;
                (j, r, c, passage.text.clone())
            }
            None => (
                Judgement::Uncertain,
                format!(
                    "locator {:?} did not resolve to any passage in source {:?}",
                    warrant.locator, warrant.source_id
                ),
                0.0,
                String::new(),
            ),
        };

        let trust_rung = match (resolved.is_some(), judgement) {
            (true, Judgement::Entailed) => TrustRung::Entailed,
            (true, _) => TrustRung::Targeted,
            (false, _) => TrustRung::Bare,
        };

        verdicts.push(Verdict {
            decl: warrant.decl.clone(),
            source_id: warrant.source_id.clone(),
            locator: warrant.locator.clone(),
            target_passage,
            machine_english: lean_info.type_english,
            judgement,
            trust_rung,
            rationale,
            confidence,
        });
    }
    Ok(verdicts)
}

/// Test-mode entry point: always bypasses the Lean-exe spawn by parsing
/// `lean_info_override` (a captured [`lean::LeanDeclInfo`] JSON fixture),
/// so it never spawns `lake` or touches the network regardless of `stub`.
/// This is what `end_to_end_stub_produces_entailed_verdict` calls.
pub fn run_check_for_test(
    manifest: &Path,
    lean_info_override: &Path,
    stub: bool,
) -> anyhow::Result<Vec<Verdict>> {
    run_check(manifest, Some(lean_info_override), Path::new("lean"), stub)
}
