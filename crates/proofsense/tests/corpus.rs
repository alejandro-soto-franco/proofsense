//! Verification against a real transcribed corpus.
//!
//! The sources these tests read are copyrighted textbooks and are not in this
//! repository, so every test here is `#[ignore]`d and reads its input from an
//! environment variable. CI never runs them and never needs the files. This
//! matches how the discrimination set is handled: measured locally, reported
//! in the README, absent from the tree.
//!
//! Run against a MinerU Markdown transcription with:
//!
//! ```text
//! PROOFSENSE_CORPUS_EVANS=/path/to/evans-2010-par-dif-equ.md \
//!   cargo test --test corpus -- --ignored --nocapture
//! ```
//!
//! The expected values below were measured on 2026-07-27 against the
//! transcription under `elliptic-pdes/latex/litreview/sources/`. They pin the
//! parser against real input, where the fixtures pin it against the line forms
//! alone.

use proofsense::ingest::{load_passages_markdown, Passage};
use std::path::PathBuf;

/// The corpus path, or `None` when the variable is unset.
fn corpus() -> Option<PathBuf> {
    std::env::var_os("PROOFSENSE_CORPUS_EVANS").map(PathBuf::from)
}

fn load() -> Vec<Passage> {
    let path = corpus().expect("set PROOFSENSE_CORPUS_EVANS to the transcription path");
    load_passages_markdown(&path).expect("parsing the corpus")
}

/// Section locators, in document order, deduplicated from statement passages.
fn sections(passages: &[Passage]) -> Vec<&str> {
    passages
        .iter()
        .map(|p| p.locator.as_str())
        .filter(|l| !l.contains(' '))
        .collect()
}

fn number(locator: &str) -> Vec<u32> {
    locator.split('.').filter_map(|c| c.parse().ok()).collect()
}

#[test]
#[ignore = "needs a corpus that cannot be committed"]
fn heading_recovery_finds_every_section_and_never_regresses() {
    let passages = load();
    let sections = sections(&passages);

    // Measured 2026-07-27: 232 headings across 35,493 lines.
    assert_eq!(sections.len(), 232, "section count changed");

    // A section-number sequence that never decreases is the evidence the
    // heading rule is picking up structure rather than noise. A numbered list
    // item admitted as a heading shows up here immediately.
    let regressions: Vec<(&str, &str)> = sections
        .windows(2)
        .filter(|w| number(w[1]) <= number(w[0]))
        .map(|w| (w[0], w[1]))
        .collect();
    assert!(
        regressions.is_empty(),
        "non-monotone heading transitions: {regressions:?}"
    );
}

#[test]
#[ignore = "needs a corpus that cannot be committed"]
fn the_cited_section_carries_three_theorems() {
    let passages = load();
    let statements: Vec<&str> = passages
        .iter()
        .map(|p| p.locator.as_str())
        .filter(|l| l.starts_with("6.3.1 "))
        .collect();

    // Interior H2-regularity, Higher interior regularity, and Infinite
    // differentiability in the interior.
    assert_eq!(
        statements,
        vec!["6.3.1 Thm 1", "6.3.1 Thm 2", "6.3.1 Thm 3"]
    );
}

/// The measurement this whole sub-project rests on: resolving to the statement
/// rather than the section shrinks the operand by an order of magnitude, and
/// the smaller operand is the theorem the declaration formalises.
#[test]
#[ignore = "needs a corpus that cannot be committed"]
fn the_statement_operand_is_an_order_of_magnitude_smaller_than_the_section() {
    let passages = load();
    let find = |locator: &str| {
        passages
            .iter()
            .find(|p| p.locator == locator)
            .unwrap_or_else(|| panic!("no passage {locator:?}"))
    };

    let section = find("6.3.1").text.len();
    let statement = find("6.3.1 Thm 1").text.len();

    println!("section 6.3.1: {section} chars");
    println!("statement 6.3.1 Thm 1: {statement} chars");
    println!("ratio: {:.1}x", section as f64 / statement as f64);

    // Measured 2026-07-27: 12,540 against 1,298, a factor of 9.7. The bounds
    // are loose enough to survive whitespace handling changing slightly, and
    // tight enough to fail if the statement stops being a statement.
    assert!(
        (1_000..2_000).contains(&statement),
        "statement was {statement} chars"
    );
    assert!(
        section > statement * 5,
        "section {section} vs statement {statement}"
    );

    // It is the right theorem.
    let text = &find("6.3.1 Thm 1").text;
    assert!(text.contains("Interior"), "not the interior theorem");
    assert!(
        !text.contains("Higher interior regularity"),
        "statement leaked into Theorem 2"
    );
}
