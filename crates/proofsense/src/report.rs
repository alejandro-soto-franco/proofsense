//! The run report: verdicts plus what was checked and what did the checking.
//!
//! A verdict on its own says nothing about which transcription it was checked
//! against, which model judged it, or which revision of the subject was
//! elaborated. The report pins all three, so a result is reproducible and a
//! submission gate has something to verify.

use crate::verdict::Verdict;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The report schema identifier. Bump the version on any breaking change to
/// the shape below.
pub const SCHEMA: &str = "proofsense.report/1";

/// One Lean package and the revision it was pinned at.
#[derive(Debug, Clone, Serialize)]
pub struct LeanPackage {
    pub name: String,
    pub rev: String,
}

/// Run-level facts: what was read and what it was elaborated against.
#[derive(Debug, Clone, Serialize)]
pub struct RunInfo {
    /// SHA-256 of the manifest file's bytes.
    pub manifest_sha256: String,
    /// Contents of `lean-toolchain`, when the Lake project is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lean_toolchain: Option<String>,
    /// The package the manifest's imports come from, when it can be matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<LeanPackage>,
    /// Every package pinned in the Lake manifest. Mathlib's revision changes
    /// elaboration, so pinning the subject alone would under-record the run.
    pub lean_packages: Vec<LeanPackage>,
}

/// Which judge ran, and how it was configured.
#[derive(Debug, Clone, Serialize)]
pub struct JudgeInfo {
    /// `"llm"` or `"stub"`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The floor actually applied when deriving relations.
    pub confidence_floor: f32,
    /// SHA-256 of the prompt template. A prompt change changes verdicts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_sha256: Option<String>,
}

/// One literature source as it was actually read.
#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    pub id: String,
    pub content_list_sha256: String,
    pub passage_count: usize,
}

/// A complete run.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub proofsense_version: &'static str,
    pub run: RunInfo,
    pub judge: JudgeInfo,
    pub sources: Vec<SourceInfo>,
    pub verdicts: Vec<Verdict>,
}

/// Render the run and judge identity as human-readable CLI text, printed once
/// above the per-verdict diagnostics so a reader knows what produced them.
pub fn render_header(report: &Report) -> String {
    let subject = match &report.run.subject {
        Some(p) => format!("{} @ {}", p.name, p.rev),
        None => "unidentified".to_string(),
    };
    let model = report.judge.model.as_deref().unwrap_or("n/a");
    let effort = report.judge.effort.as_deref().unwrap_or("n/a");

    format!(
        "proofsense {version} ({schema})\n\
         manifest: {manifest_sha256}\n\
         subject: {subject}\n\
         toolchain: {toolchain}\n\
         judge: {kind} model={model} effort={effort} floor={floor:.2}\n",
        version = report.proofsense_version,
        schema = report.schema,
        manifest_sha256 = report.run.manifest_sha256,
        subject = subject,
        toolchain = report.run.lean_toolchain.as_deref().unwrap_or("unknown"),
        kind = report.judge.kind,
        model = model,
        effort = effort,
        floor = report.judge.confidence_floor,
    )
}

#[derive(Debug, Deserialize)]
struct LakeManifest {
    #[serde(default)]
    packages: Vec<LakePackage>,
}

#[derive(Debug, Deserialize)]
struct LakePackage {
    name: String,
    #[serde(default)]
    rev: Option<String>,
}

/// Parse the `packages` array of a `lake-manifest.json`. Entries without a
/// revision (a local path dependency, say) are skipped, since there is nothing
/// to pin.
pub fn parse_lake_manifest(raw: &str) -> anyhow::Result<Vec<LeanPackage>> {
    let manifest: LakeManifest = serde_json::from_str(raw).context("parsing lake-manifest.json")?;
    Ok(manifest
        .packages
        .into_iter()
        .filter_map(|p| p.rev.map(|rev| LeanPackage { name: p.name, rev }))
        .collect())
}

/// Identify the subject package: the one whose name matches the root component
/// of the first subject import. Returns `None` when nothing matches, rather
/// than guessing.
pub fn subject_package(
    packages: &[LeanPackage],
    subject_imports: &[String],
) -> Option<LeanPackage> {
    let root = subject_imports.first()?.split('.').next()?;
    packages.iter().find(|p| p.name == root).cloned()
}

/// Read `lean-toolchain` and `lake-manifest.json` from `lean_dir`. Both are
/// optional: a run driven entirely by `--lean-info` need not have the Lake
/// project present, and a missing file records as absent rather than failing
/// the run.
pub fn read_lean_provenance(
    lean_dir: &Path,
    subject_imports: &[String],
) -> (Option<String>, Option<LeanPackage>, Vec<LeanPackage>) {
    let toolchain = std::fs::read_to_string(lean_dir.join("lean-toolchain"))
        .ok()
        .map(|s| s.trim().to_string());

    let packages = std::fs::read_to_string(lean_dir.join("lake-manifest.json"))
        .ok()
        .and_then(|raw| parse_lake_manifest(&raw).ok())
        .unwrap_or_default();

    let subject = subject_package(&packages, subject_imports);
    (toolchain, subject, packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lean_packages_are_read_from_a_lake_manifest() {
        let raw = r#"{
          "version": "1.2.0",
          "packages": [
            {"name": "EllipticPdes", "rev": "7c3ebf21", "type": "git"},
            {"name": "mathlib", "rev": "542645ab", "type": "git"}
          ]
        }"#;
        let packages = parse_lake_manifest(raw).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "EllipticPdes");
        assert_eq!(packages[0].rev, "7c3ebf21");
    }

    #[test]
    fn the_subject_is_the_package_matching_the_import_root() {
        let packages = vec![
            LeanPackage {
                name: "EllipticPdes".to_string(),
                rev: "7c3ebf21".to_string(),
            },
            LeanPackage {
                name: "mathlib".to_string(),
                rev: "542645ab".to_string(),
            },
        ];
        let subject = subject_package(&packages, &["EllipticPdes.Regularity.Interior".to_string()]);
        assert_eq!(subject.unwrap().name, "EllipticPdes");
    }

    #[test]
    fn an_unmatched_import_root_yields_no_subject() {
        let packages = vec![LeanPackage {
            name: "mathlib".to_string(),
            rev: "542645ab".to_string(),
        }];
        assert!(subject_package(&packages, &["Nowhere.At.All".to_string()]).is_none());
        assert!(subject_package(&packages, &[]).is_none());
    }

    fn sample_report() -> Report {
        Report {
            schema: SCHEMA,
            proofsense_version: env!("CARGO_PKG_VERSION"),
            run: RunInfo {
                manifest_sha256: "9f2a".to_string(),
                lean_toolchain: None,
                subject: None,
                lean_packages: Vec::new(),
            },
            judge: JudgeInfo {
                kind: "stub",
                model: None,
                effort: None,
                confidence_floor: 0.0,
                prompt_sha256: None,
            },
            sources: Vec::new(),
            verdicts: Vec::new(),
        }
    }

    #[test]
    fn the_schema_and_version_are_stamped() {
        let json = serde_json::to_value(sample_report()).unwrap();
        assert_eq!(json["schema"], "proofsense.report/1");
        assert_eq!(json["proofsense_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["judge"]["kind"], "stub");
        assert!(json["judge"].get("model").is_none());
    }

    #[test]
    fn the_header_names_the_judge_and_what_it_read() {
        let text = render_header(&sample_report());
        assert!(text.contains("proofsense.report/1"));
        assert!(text.contains("9f2a"));
        assert!(text.contains("judge: stub"));
        assert!(text.contains("floor=0.00"));
        assert!(text.contains("subject: unidentified"));
    }
}
