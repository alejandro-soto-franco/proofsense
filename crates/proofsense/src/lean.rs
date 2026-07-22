//! Rust ↔ Lean-exe bridge: spawn `lake exe proofsense-lean` and parse its
//! one-line JSON output.
//!
//! `parse_decl_info` is pure (no process spawn) so it can be unit-tested
//! directly against a captured fixture. `extract_decl` is the live path that
//! actually invokes the Lean exe; it is exercised end-to-end elsewhere, not
//! under unit test here.

use anyhow::{bail, Context};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// One declaration's extracted, delaborated info, as emitted by
/// `lake exe proofsense-lean`. Field names match the Lean JSON keys exactly.
#[derive(Debug, Clone, Deserialize)]
pub struct LeanDeclInfo {
    pub decl: String,
    pub type_pp: String,
    pub type_english: String,
    pub axioms: Vec<String>,
    pub sorry_free: bool,
}

/// Parse a single JSON-object line (the Lean exe's stdout) into a
/// [`LeanDeclInfo`]. Pure: does not spawn anything, so it is unit-testable
/// against a captured fixture.
pub fn parse_decl_info(json_line: &str) -> anyhow::Result<LeanDeclInfo> {
    serde_json::from_str(json_line)
        .with_context(|| format!("parsing Lean decl-info JSON: {json_line}"))
}

/// Run `lake exe proofsense-lean --decl <decl> --imports <A,B,...>` in
/// `lean_dir`, capture stdout, and parse its last non-empty line as a
/// [`LeanDeclInfo`]. A non-zero exit status (or spawn failure) surfaces as an
/// `anyhow` error carrying stderr.
pub fn extract_decl(
    lean_dir: &Path,
    decl: &str,
    imports: &[String],
) -> anyhow::Result<LeanDeclInfo> {
    let output = Command::new("lake")
        .args([
            "exe",
            "proofsense-lean",
            "--decl",
            decl,
            "--imports",
            &imports.join(","),
        ])
        .current_dir(lean_dir)
        .output()
        .with_context(|| {
            format!(
                "spawning `lake exe proofsense-lean` in {}",
                lean_dir.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "lake exe proofsense-lean exited with {}: {}",
            output.status,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .with_context(|| format!("lake exe proofsense-lean produced no output for decl {decl}"))?;

    parse_decl_info(last_line.trim())
}
