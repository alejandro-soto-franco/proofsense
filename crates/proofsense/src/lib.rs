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
pub mod report;
pub mod verdict;

use anyhow::Context;
use entail::{Entailment, LlmEntailment, StubEntailment};
use manifest::Manifest;
use report::{Report, RunInfo, SourceInfo};
use std::path::Path;
use verdict::{classify, Defect, Relation, TrustRung, Verdict};

/// Run the full pipeline (ingest -> resolve -> Lean bridge -> entailment ->
/// verdict) for every warrant in `manifest_path`, returning a [`Report`]
/// carrying one [`Verdict`] per warrant plus what was read and what judged
/// it.
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
/// yields [`TrustRung::Bare`] (a citation only, see the labelling rule in
/// `verdict.rs`); otherwise the two directional entailment answers are
/// combined into a [`Relation`], and the rung comes from
/// [`classify`]`(warrant.claim, relation)`.
pub fn run_check(
    manifest_path: &Path,
    lean_info_override: Option<&Path>,
    lean_dir: &Path,
    stub: bool,
) -> anyhow::Result<Report> {
    let entailment: Box<dyn Entailment> = if stub {
        Box::new(StubEntailment::default())
    } else {
        Box::new(LlmEntailment::from_env()?)
    };
    run_check_with(
        manifest_path,
        lean_info_override,
        lean_dir,
        entailment.as_ref(),
    )
}

/// Core of [`run_check`], with the entailment backend supplied by the caller.
///
/// Exposed so tests can drive the pipeline with a backend the stub cannot
/// impersonate. [`entail::StubEntailment`] is symmetric by construction, so
/// without this seam nothing can tell a correct direction order from a
/// transposed one. Not part of the supported API.
#[doc(hidden)]
pub fn run_check_with(
    manifest_path: &Path,
    lean_info_override: Option<&Path>,
    lean_dir: &Path,
    entailment: &dyn Entailment,
) -> anyhow::Result<Report> {
    let raw = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .with_context(|| format!("parsing manifest JSON {}", manifest_path.display()))?;
    let manifest_sha256 = hash::sha256_hex(raw.as_bytes());

    // Paths inside the manifest (e.g. each source's `content_list`) are
    // resolved relative to the manifest file's own directory, not the
    // process's current directory. This keeps a manifest portable: it works
    // identically whether invoked from the crate root (as `cargo test`
    // does) or from the workspace root (as `cargo run` does).
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    // The backend describes its own identity: a report can only ever record
    // the judge that actually ran, never one a caller merely claims.
    let judge = entailment.describe();

    // Load each source's passages once, keyed by id, rather than per
    // warrant, and record what was actually read.
    let mut sources: Vec<SourceInfo> = Vec::new();
    let mut loaded: std::collections::HashMap<String, Vec<ingest::Passage>> =
        std::collections::HashMap::new();
    for source in &manifest.sources {
        let content_list_path = if source.content_list.is_absolute() {
            source.content_list.clone()
        } else {
            manifest_dir.join(&source.content_list)
        };
        let bytes = std::fs::read(&content_list_path)
            .with_context(|| format!("reading content_list {}", content_list_path.display()))?;
        let (passages, format) = ingest::load_source(&content_list_path)?;
        sources.push(SourceInfo {
            id: source.id.clone(),
            content_list_sha256: hash::sha256_hex(&bytes),
            passage_count: passages.len(),
            format: format.as_str(),
        });
        loaded.insert(source.id.clone(), passages);
    }

    let mut verdicts = Vec::with_capacity(manifest.warrants.len());
    for warrant in &manifest.warrants {
        let passages = loaded.get(&warrant.source_id).with_context(|| {
            format!(
                "warrant for {:?} cites unknown source_id {:?}",
                warrant.decl, warrant.source_id
            )
        })?;
        let resolved = locator::resolve(passages, &warrant.locator);

        let lean_info = match lean_info_override {
            Some(path) => {
                let s = std::fs::read_to_string(path)
                    .with_context(|| format!("reading lean-info override {}", path.display()))?;
                lean::parse_decl_info(s.trim())?
            }
            None => lean::extract_decl(lean_dir, &warrant.decl, &manifest.subject_imports)?,
        };

        let (relation, source_entails_decl, decl_entails_source, target_passage, passage_sha256) =
            match resolved {
                Some(passage) => {
                    let check = entailment.check(&passage.text, &lean_info.type_english)?;
                    let relation = Relation::derive(
                        &check.source_entails_decl,
                        &check.decl_entails_source,
                        entailment.confidence_floor(),
                    );
                    (
                        relation,
                        Some(check.source_entails_decl),
                        Some(check.decl_entails_source),
                        passage.text.clone(),
                        Some(hash::sha256_hex(passage.text.as_bytes())),
                    )
                }
                // No passage resolved, so there is nothing to derive a
                // relation from and the warrant stays a bare citation.
                None => (Relation::Undetermined, None, None, String::new(), None),
            };

        let (trust_rung, defect): (TrustRung, Option<Defect>) = if source_entails_decl.is_some() {
            classify(warrant.claim, relation)
        } else {
            (TrustRung::Bare, None)
        };

        verdicts.push(Verdict {
            decl: warrant.decl.clone(),
            source_id: warrant.source_id.clone(),
            locator: warrant.locator.clone(),
            claim: warrant.claim,
            relation,
            trust_rung,
            defect,
            source_entails_decl,
            decl_entails_source,
            decl_axioms: lean_info.axioms.clone(),
            sorry_free: lean_info.sorry_free,
            passage_sha256,
            machine_english: lean_info.type_english,
            target_passage,
        });
    }

    let (lean_toolchain, subject, lean_packages) =
        report::read_lean_provenance(lean_dir, &manifest.subject_imports);

    Ok(Report {
        schema: report::SCHEMA,
        proofsense_version: env!("CARGO_PKG_VERSION"),
        run: RunInfo {
            manifest_sha256,
            lean_toolchain,
            subject,
            lean_packages,
        },
        judge,
        sources,
        verdicts,
    })
}

/// Test-mode entry point: always bypasses the Lean-exe spawn by parsing
/// `lean_info_override` (a captured [`lean::LeanDeclInfo`] JSON fixture),
/// so it never spawns `lake` or touches the network regardless of `stub`.
/// This is what `end_to_end_stub_produces_a_symmetric_relation` calls.
pub fn run_check_for_test(
    manifest: &Path,
    lean_info_override: &Path,
    stub: bool,
) -> anyhow::Result<Report> {
    run_check(manifest, Some(lean_info_override), Path::new("lean"), stub)
}
