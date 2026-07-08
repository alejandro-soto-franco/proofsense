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
    assert_eq!(
        info.decl,
        "EllipticDirichlet.Regularity.interior_H2_estimate"
    );
    assert!(info.sorry_free);
    assert!(!info.type_english.is_empty());
}

#[test]
fn stub_entailment_yields_entailed_verdict() {
    use proofsense::entail::{Entailment, StubEntailment};
    let (j, _r, _c) = StubEntailment::default()
        .check(
            "the second weak derivatives exist in L^2 with the bound",
            "for every compact set the second weak derivative exists in L^2 ...",
        )
        .unwrap();
    assert!(matches!(j, proofsense::verdict::Judgement::Entailed));
}

#[test]
fn end_to_end_stub_produces_entailed_verdict() {
    let out = proofsense::run_check_for_test(
        std::path::Path::new("tests/fixtures/manifest.json"),
        std::path::Path::new("tests/fixtures/interior_h2.leaninfo.json"),
        /*stub=*/ true,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert!(matches!(
        out[0].trust_rung,
        proofsense::verdict::TrustRung::Entailed | proofsense::verdict::TrustRung::Targeted
    ));
    assert_eq!(
        out[0].decl,
        "EllipticDirichlet.Regularity.interior_H2_estimate"
    );
}
