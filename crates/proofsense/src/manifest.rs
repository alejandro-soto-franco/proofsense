//! Manifest types: the input file describing which declarations ("warrants")
//! to check against which literature sources.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What a warrant claims about the correspondence it asserts. The two
/// readings fail in different directions, so the same relation between
/// source and declaration is a defect under one and a pass under the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Claim {
    /// This declaration IS the cited theorem. The stricter reading, and the
    /// default when a manifest does not say.
    #[default]
    Formalises,
    /// This step is justified by the source. A declaration that is a special
    /// case of the cited result still satisfies this.
    FollowsFrom,
}

impl std::fmt::Display for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Claim::Formalises => "formalises",
            Claim::FollowsFrom => "follows_from",
        };
        f.write_str(s)
    }
}

/// One warrant: a claim that a Lean declaration corresponds to a passage in
/// a literature source, addressed by a structural locator (e.g. "§6.3.1").
#[derive(Debug, Clone, Deserialize)]
pub struct Warrant {
    pub decl: String,
    pub source_id: String,
    pub locator: String,
    /// What this warrant claims. Absent in a manifest, it reads as
    /// [`Claim::Formalises`].
    #[serde(default)]
    pub claim: Claim,
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
    /// Maps a Lean identifier to the function space it denotes, e.g.
    /// `"H01": "H1_0"`. Used by [`crate::structure`] to compare the spaces a
    /// passage names against those its declaration names.
    ///
    /// The table lives here rather than in the tool because the names are a
    /// development's own vocabulary, and because its author is the one who can
    /// be held to it. Absent, the structural comparison reports the passage's
    /// spaces and none of the declaration's.
    #[serde(default)]
    pub space_map: crate::structure::SpaceMap,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_display_matches_the_serialised_form() {
        assert_eq!(Claim::Formalises.to_string(), "formalises");
        assert_eq!(Claim::FollowsFrom.to_string(), "follows_from");
        assert_eq!(
            serde_json::to_value(Claim::FollowsFrom).unwrap(),
            serde_json::json!("follows_from")
        );
    }

    #[test]
    fn a_warrant_without_a_claim_reads_as_formalises() {
        let w: Warrant =
            serde_json::from_str(r#"{"decl":"A.b","source_id":"s","locator":"§1.1"}"#).unwrap();
        assert_eq!(w.claim, Claim::Formalises);
    }
}
