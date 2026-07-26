#[test]
fn ingest_groups_section_passages() {
    let p = proofsense::ingest::load_passages(std::path::Path::new(
        "tests/fixtures/evans-6-3-1.content_list.json",
    ))
    .unwrap();
    assert!(p.iter().any(|x| x.locator.contains("6.3.1")));
    let s = p.iter().find(|x| x.locator.contains("6.3.1")).unwrap();
    assert!(!s.text.is_empty());
}

#[test]
fn resolve_section_locator() {
    let p = proofsense::ingest::load_passages(std::path::Path::new(
        "tests/fixtures/evans-6-3-1.content_list.json",
    ))
    .unwrap();
    let hit = proofsense::locator::resolve(&p, "§6.3.1").expect("resolved");
    assert!(hit.locator.contains("6.3.1"));
}

#[test]
fn parse_lean_decl_info() {
    let s = std::fs::read_to_string("tests/fixtures/interior_h2.leaninfo.json").unwrap();
    let info = proofsense::lean::parse_decl_info(s.trim()).unwrap();
    assert_eq!(info.decl, "EllipticPdes.Regularity.interior_H2_estimate");
    assert!(info.sorry_free);
    assert!(!info.type_english.is_empty());
}

#[test]
fn stub_answers_both_directions_identically() {
    use proofsense::entail::{Entailment, StubEntailment};
    let check = StubEntailment::default()
        .check(
            "the second weak derivatives exist in L^2 with the bound",
            "for every compact set the second weak derivative exists in L^2 ...",
        )
        .unwrap();
    assert!(check.source_entails_decl.holds);
    assert_eq!(
        check.source_entails_decl.holds,
        check.decl_entails_source.holds
    );
    assert!(check.source_entails_decl.rationale.starts_with("stub:"));
}

/// The stub computes symmetric lexical overlap, so it cannot see a one-sided
/// relation. Its floor is zero because its confidence is an overlap ratio
/// rather than a calibrated probability.
#[test]
fn stub_can_only_produce_equivalent_or_divergent() {
    use proofsense::entail::{Entailment, StubEntailment};
    use proofsense::verdict::Relation;

    let stub = StubEntailment::default();
    assert_eq!(stub.confidence_floor(), 0.0);

    for (passage, english) in [
        (
            "the second weak derivatives exist in L^2 with the bound",
            "for every compact set the second weak derivative exists in L^2 ...",
        ),
        ("completely unrelated text about cats", "elliptic operators"),
    ] {
        let check = stub.check(passage, english).unwrap();
        let relation = Relation::derive(
            &check.source_entails_decl,
            &check.decl_entails_source,
            stub.confidence_floor(),
        );
        assert!(
            matches!(relation, Relation::Equivalent | Relation::Divergent),
            "stub produced {relation}"
        );
    }
}

#[test]
fn end_to_end_stub_produces_a_symmetric_relation() {
    use proofsense::verdict::Relation;

    let out = proofsense::run_check_for_test(
        std::path::Path::new("tests/fixtures/manifest.json"),
        std::path::Path::new("tests/fixtures/interior_h2.leaninfo.json"),
        /*stub=*/ true,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].decl, "EllipticPdes.Regularity.interior_H2_estimate");
    assert!(
        matches!(out[0].relation, Relation::Equivalent | Relation::Divergent),
        "the stub cannot see a one-sided relation, got {}",
        out[0].relation
    );
}

/// A `Prop` hypothesis the statement actually uses must be *bound* in the
/// rendering. Dropping its name, as the implication reading does, would leave
/// that name free in the English and the rendering would no longer be a
/// faithful reading of the term. A hypothesis the statement does not use still
/// reads as a plain implication.
#[test]
fn dependent_prop_hypotheses_are_bound_not_elided() {
    let s = std::fs::read_to_string("tests/fixtures/interior_h2.leaninfo.json").unwrap();
    let info = proofsense::lean::parse_decl_info(s.trim()).unwrap();
    let english = &info.type_english;

    // `hOm` is projected later in the statement, so it must be named.
    assert!(
        english.contains("for all h\u{3a9}m :"),
        "a used Prop hypothesis was elided, leaving its name free: {english}"
    );
    // Openness of the domain is never referred to again, so it stays an implication.
    assert!(
        english.contains("if \u{3a9} is open, then"),
        "an unused Prop hypothesis should still read as an implication: {english}"
    );
    // No bare projection off a name that was never introduced.
    assert!(
        !english.contains(", \u{2016}h\u{3a9}m"),
        "found a projection off an unbound hypothesis: {english}"
    );
}
