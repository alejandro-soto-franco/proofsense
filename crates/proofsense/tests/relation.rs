//! The five-outcome discrimination set. Passages are authored here, so the
//! expected relation for each declaration holds by construction.
//!
//! These cases run against the stub, which is symmetric and therefore cannot
//! see a one-sided relation. What they pin is the wiring: the claim reaches
//! `classify`, an unresolved locator yields `Bare`, and the report carries the
//! per-source provenance. The one-sided cases are exercised against the real
//! judge in the private Evans set, which does not run in CI.

use proofsense::verdict::{Defect, Relation, TrustRung};
use std::path::Path;

fn run(manifest: &str, lean_info: &str) -> proofsense::report::Report {
    proofsense::run_check_for_test(
        Path::new(&format!("tests/fixtures/{manifest}")),
        Path::new(&format!("tests/fixtures/synthetic/{lean_info}")),
        /*stub=*/ true,
    )
    .unwrap()
}

#[test]
fn a_matching_statement_reaches_entailed() {
    let report = run("synthetic-manifest.json", "equivalent.leaninfo.json");
    let v = &report.verdicts[0];
    assert_eq!(v.relation, Relation::Equivalent);
    assert_eq!(v.trust_rung, TrustRung::Entailed);
    assert!(v.defect.is_none());
}

#[test]
fn a_different_theorem_is_divergent_and_unsupported() {
    let report = run("synthetic-manifest.json", "divergent.leaninfo.json");
    let v = &report.verdicts[0];
    assert_eq!(v.relation, Relation::Divergent);
    assert_eq!(v.trust_rung, TrustRung::Targeted);
    assert_eq!(v.defect, Some(Defect::Unsupported));
}

/// The two one-sided fixtures exist because P2 perturbs them and the private
/// Evans set exercises them against the real judge. Here they pin the stub's
/// blindness: a symmetric backend must never report a one-sided relation, so
/// CI can state plainly which outcomes it does not cover.
#[test]
fn the_stub_cannot_see_a_one_sided_relation() {
    for fixture in [
        "decl_specialises.leaninfo.json",
        "decl_exceeds.leaninfo.json",
    ] {
        let report = run("synthetic-manifest.json", fixture);
        assert!(
            !matches!(
                report.verdicts[0].relation,
                Relation::DeclSpecialises | Relation::DeclExceeds
            ),
            "the symmetric stub reported a one-sided relation on {fixture}"
        );
    }
}

#[test]
fn an_unresolved_locator_is_bare_and_has_no_directions() {
    // The locator is what fails here, so the statement does not matter.
    let report = run(
        "synthetic-manifest-unresolved.json",
        "equivalent.leaninfo.json",
    );
    let v = &report.verdicts[0];
    assert_eq!(v.trust_rung, TrustRung::Bare);
    assert!(v.defect.is_none());
    assert!(v.source_entails_decl.is_none());
    assert!(v.passage_sha256.is_none());
}

#[test]
fn the_claim_is_carried_from_the_manifest_to_the_verdict() {
    use proofsense::manifest::Claim;
    let report = run(
        "synthetic-manifest-follows-from.json",
        "equivalent.leaninfo.json",
    );
    assert_eq!(report.verdicts[0].claim, Claim::FollowsFrom);
}

#[test]
fn the_report_records_the_synthetic_source() {
    let report = run("synthetic-manifest.json", "equivalent.leaninfo.json");
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].id, "synthetic");
    assert_eq!(report.sources[0].passage_count, 3);
}

/// The divergent fixture must diverge by a comfortable margin, not by a token
/// or two. It previously shared `a` and `b` with the passage and cleared an
/// absolute threshold of three only just; when the stub moved to a coverage
/// rule that margin vanished and the case silently became `Equivalent`.
#[test]
fn the_divergent_fixture_shares_no_vocabulary_with_the_passage() {
    let report = run("synthetic-manifest.json", "divergent.leaninfo.json");
    let v = &report.verdicts[0];
    let rationale = &v.source_entails_decl.as_ref().unwrap().rationale;

    let shared: usize = rationale
        .split_whitespace()
        .find_map(|t| t.split('/').next()?.parse().ok())
        .expect("stub rationale reports shared/total");

    assert_eq!(
        shared, 0,
        "divergent fixture shares {shared} token(s) with the passage: {rationale}"
    );
}
