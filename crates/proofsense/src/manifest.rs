//! Manifest types: the input file describing which declarations ("warrants")
//! to check against which literature sources.

use serde::Deserialize;
use std::path::PathBuf;

/// One warrant: a claim that a Lean declaration corresponds to a passage in
/// a literature source, addressed by a structural locator (e.g. "§6.3.1").
#[derive(Debug, Clone, Deserialize)]
pub struct Warrant {
    pub decl: String,
    pub source_id: String,
    pub locator: String,
}

/// A literature source: an id and the path to its MinerU `content_list.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub id: String,
    pub content_list: PathBuf,
}

/// The top-level manifest: which Lean modules to import, which sources are
/// available, and which warrants to check.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub subject_imports: Vec<String>,
    pub warrants: Vec<Warrant>,
    pub sources: Vec<Source>,
}
