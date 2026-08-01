//! Literature ingest: turn a transcribed source into locator-addressable
//! passages, at section and at statement granularity.
//!
//! Two input formats are read, dispatched by file extension in
//! [`load_source`]:
//!
//! - `.json`, a MinerU `content_list.json` (pipeline backend, legacy flat
//!   format): a JSON array of block objects. Relevant fields (confirmed
//!   against the MinerU docs, `docs/en/reference/output_files.md`):
//!     - `type`: "text" | "equation" | "image" | "table" | ...
//!     - `text`: the block's textual content (for text/equation blocks). For
//!       equation blocks the LaTeX lives here, wrapped in `$$...$$`.
//!     - `text_level`: present (>= 1) only on heading blocks; absent or 0 on
//!       ordinary body text. A heading is `type:"text"` with `text_level` set.
//!     - `page_idx`, `bbox`, `text_format`, `img_path`, ...: ignored here.
//!
//!   Unknown fields are tolerated (serde ignores them).
//!
//! - `.md`, MinerU's Markdown export, which is what `mineru_transcribe.py`
//!   actually writes. This export carries no `#` heading markers at all, so
//!   structure is recovered from the line forms described on
//!   [`load_passages_markdown`].
//!
//! # Granularity
//!
//! A locator naming a section resolves to every theorem under that heading. A
//! declaration formalising one of them is then asked to entail all of them,
//! which is answered false almost always, so
//! [`crate::verdict::Relation::Equivalent`] becomes unreachable and the
//! [`crate::verdict::Defect::Understated`] finding fires on faithful work. The
//! Markdown loader therefore emits a passage per *statement* alongside the
//! section passage, so a warrant can cite the theorem it means.
//!
//! Measured on Evans, *Partial Differential Equations*: section 6.3.1 parses
//! to 12,573 characters carrying three theorems, and its Theorem 1 to 1,306,
//! an operand 9.6 times smaller. `tests/corpus.rs` pins these against the real
//! transcription.

use anyhow::Context;
use serde::Deserialize;
use std::path::Path;

/// A locator-addressable passage: either the text under one section heading,
/// or one statement within such a section, plus any display equations that
/// appeared inside it (as raw LaTeX).
#[derive(Debug, Clone)]
pub struct Passage {
    /// Structural locator for the passage: `"6.3.1"` for a section, or
    /// `"6.3.1 Thm 1"` for a statement within one.
    pub locator: String,
    /// Concatenated body text (prose + inline equation LaTeX).
    pub text: String,
    /// Display-equation LaTeX blocks appearing in the passage, in order.
    pub latex_math: Vec<String>,
}

/// Which parser produced a source's passages. Recorded on the report so a run
/// states how its passages were obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// MinerU `content_list.json`.
    ContentList,
    /// MinerU Markdown export.
    Markdown,
}

impl SourceFormat {
    /// The stable string recorded on the report.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceFormat::ContentList => "content_list",
            SourceFormat::Markdown => "markdown",
        }
    }

    /// Dispatch on file extension: `.md` is Markdown, anything else is a
    /// content list. Defaulting rather than erroring keeps every manifest
    /// written before this loader existed working unchanged.
    pub fn of_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("md") => SourceFormat::Markdown,
            _ => SourceFormat::ContentList,
        }
    }
}

/// Load a source, dispatching on file extension, and report which parser ran.
pub fn load_source(path: &Path) -> anyhow::Result<(Vec<Passage>, SourceFormat)> {
    let format = SourceFormat::of_path(path);
    let passages = match format {
        SourceFormat::ContentList => load_passages(path)?,
        SourceFormat::Markdown => load_passages_markdown(path)?,
    };
    Ok((passages, format))
}

/// One MinerU block. Only the fields we consume are named; the rest are
/// ignored via serde's default `deny_unknown_fields = false` behaviour.
#[derive(Debug, Deserialize)]
struct Block {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
    /// Present only on headings. `Some(n)` with `n >= 1` marks a heading.
    #[serde(default)]
    text_level: Option<u32>,
}

impl Block {
    /// A block is a heading when `text_level` is present and non-zero.
    fn is_heading(&self) -> bool {
        matches!(self.text_level, Some(level) if level >= 1)
    }
}

/// Parse a leading section number (e.g. "6.3.1") from a heading's text.
/// Returns the dotted-number token if the text begins with one.
fn leading_section_number(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    // Keep the leading run of digits and dots (e.g. "6.3.1" from "6.3.1").
    let stripped: String = token
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    // Require at least one digit to count as a section number.
    if stripped.chars().any(|c| c.is_ascii_digit()) {
        Some(stripped.trim_matches('.').to_string())
    } else {
        None
    }
}

/// Parse a MinerU `content_list.json` file into section-locator-tagged
/// passages. Each heading block bearing a leading section number opens a new
/// passage; subsequent non-heading blocks append to it (text to `text`,
/// equations additionally to `latex_math`). Content before the first
/// recognised heading is discarded.
pub fn load_passages(content_list: &Path) -> anyhow::Result<Vec<Passage>> {
    let raw = std::fs::read_to_string(content_list)
        .with_context(|| format!("reading content_list {}", content_list.display()))?;
    let blocks: Vec<Block> = serde_json::from_str(&raw)
        .with_context(|| format!("parsing content_list JSON {}", content_list.display()))?;

    let mut passages: Vec<Passage> = Vec::new();

    for block in &blocks {
        if block.is_heading() {
            if let Some(locator) = leading_section_number(&block.text) {
                passages.push(Passage {
                    locator,
                    text: String::new(),
                    latex_math: Vec::new(),
                });
            }
            // Headings without a section number (or before one) are skipped;
            // they never contribute body text to a passage.
            continue;
        }

        // Body block: append to the currently open passage, if any.
        let Some(current) = passages.last_mut() else {
            continue;
        };

        let content = block.text.trim();
        if content.is_empty() {
            continue;
        }

        if !current.text.is_empty() {
            current.text.push_str("\n\n");
        }
        current.text.push_str(content);

        if block.block_type == "equation" {
            current.latex_math.push(content.to_string());
        }
    }

    Ok(passages)
}

/// Statement markers recognised at the head of a Markdown line, paired with
/// the abbreviation the resulting statement passage carries in its locator.
/// Uppercase in the source, since that is how MinerU transcribes the running
/// heads these textbooks set theorem statements in.
const STATEMENT_MARKERS: [(&str, &str); 5] = [
    ("THEOREM", "Thm"),
    ("LEMMA", "Lem"),
    ("COROLLARY", "Cor"),
    ("DEFINITION", "Def"),
    ("REMARK", "Rmk"),
];

/// Parse a section heading of the form `6.3.1. Interior regularity.`, the
/// only form MinerU's Markdown export preserves once it has dropped every `#`
/// marker. Returns the dotted number without its trailing separator.
///
/// At least two components are required, so a numbered list item such as
/// `1. Laplace's equation` is not mistaken for a heading. That ambiguity is
/// real: allowing a single leading number admits 262 false headings on Evans
/// against 232 true ones. The cost is that chapter-level headings
/// (`6. LINEAR ELLIPTIC EQUATIONS`) are not matched, which is acceptable
/// because warrants cite sections.
fn markdown_section_number(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let run: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    // The run must end at a separating dot, leaving the number before it.
    let number = run.strip_suffix('.')?;
    let rest = &trimmed[run.len()..];

    // A title must follow, separated by whitespace.
    if !rest.starts_with(char::is_whitespace) || rest.trim().is_empty() {
        return None;
    }
    let parts: Vec<&str> = number.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(number.to_string())
}

/// Parse a section heading carrying an UNDOTTED number: `2 Upper bound for the
/// Infimum`, the form papers set their sections in.
///
/// Papers number sections at one level where textbooks number at three, so the
/// rule above finds nothing in an arXiv source and every locator into one
/// misses. Admitting a single number is what pulls in numbered list items, and
/// the trailing dot is what separates the two: an enumerator is written `1.`
/// and a paper's section number is not. Requiring its ABSENCE therefore admits
/// the heading and rejects the list, where requiring a dotted depth cannot.
/// Measured on `schygulla-2011-wil-min-pre`, whose four sections and three
/// enumerated conditions both number from one: the undotted form selects the
/// four and none of the three.
///
/// A title must follow and must open with a capital, which is how a heading is
/// set and how a sentence fragment carrying a leading numeral is not.
fn markdown_section_number_loose(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let run: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if run.is_empty() || run.ends_with('.') {
        return None;
    }
    let rest = &trimmed[run.len()..];

    // A capitalised title must follow, separated by whitespace.
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    if !rest
        .trim_start()
        .starts_with(|c: char| c.is_ascii_uppercase())
    {
        return None;
    }
    if run.split('.').any(|p| p.is_empty()) {
        return None;
    }
    Some(run)
}

/// The components of a dotted section number, for ordering candidate headings.
/// A component that does not parse drops out, so a malformed number orders by
/// what it does carry rather than panicking.
fn section_number_key(number: &str) -> Vec<u32> {
    number.split('.').filter_map(|p| p.parse().ok()).collect()
}

/// Is a candidate heading sequence strictly increasing, read as dotted numbers.
///
/// This is the test the strict rule was justified by: on Evans it yields 232
/// headings with zero non-monotone transitions, while admitting single numbers
/// interleaves 262 list items among them and the sequence stops increasing. So
/// the same test decides when the looser form is safe, rather than a second
/// hand-tuned rule.
fn strictly_increasing(numbers: &[String]) -> bool {
    numbers
        .windows(2)
        .all(|w| section_number_key(&w[0]) < section_number_key(&w[1]))
}

/// Parse a statement marker of the form `THEOREM 1 (Interior H 2-regularity).`
/// Returns the locator abbreviation and the statement number.
///
/// A number is required. Unnumbered statements stay reachable through the
/// section passage that contains them.
fn markdown_statement_marker(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim();
    // Textbooks set the marker in caps, which is how MinerU transcribes the
    // running head; papers set it in title case and number it by section, as
    // `Lemma 2.1`. Both are taken, and a lowercase `theorem` is not, so a
    // sentence resuming mid-clause does not open a statement.
    let (word, abbrev) = STATEMENT_MARKERS.iter().find_map(|(word, abbrev)| {
        let title = title_case(word);
        if trimmed.len() > word.len() && trimmed.starts_with(word) {
            Some((word.len(), *abbrev))
        } else if trimmed.len() > title.len() && trimmed.starts_with(&title) {
            Some((title.len(), *abbrev))
        } else {
            None
        }
    })?;
    let after = &trimmed[word..];
    let rest = after.trim_start();
    if rest.len() == after.len() {
        // No whitespace separated the marker from what follows, so this is a
        // longer word that merely starts with the marker.
        return None;
    }
    // A paper numbers by section, so the number may be dotted. Any trailing
    // separator is dropped so `Lemma 2.1.` and `Lemma 2.1` give one locator.
    let run: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let number = run.trim_end_matches('.');
    if number.is_empty() || !number.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some((abbrev, number.to_string()))
}

/// `THEOREM` to `Theorem`, for the title-case form papers use.
fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_string() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// A line opening a proof, which is where a statement ends.
fn is_proof_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed
        .strip_prefix("Proof")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

/// Collect a run of Markdown lines into one passage, pulling `$$`-fenced
/// display equations into `latex_math` while keeping them in `text`, so both
/// loaders present the judge with the same shape of operand.
fn build_passage(locator: String, lines: &[&str]) -> Passage {
    let mut text = String::new();
    let mut latex_math = Vec::new();
    let mut fence: Option<String> = None;

    let push_text = |text: &mut String, content: &str| {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(content);
    };

    for line in lines {
        let trimmed = line.trim();

        if let Some(buf) = fence.as_mut() {
            if trimmed == "$$" {
                let body = buf.trim().to_string();
                if !body.is_empty() {
                    push_text(&mut text, &body);
                    latex_math.push(body);
                }
                fence = None;
            } else {
                buf.push_str(trimmed);
                buf.push('\n');
            }
            continue;
        }

        if trimmed == "$$" {
            fence = Some(String::new());
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        push_text(&mut text, trimmed);
    }

    // An unterminated fence still carries content; keep it rather than
    // dropping the tail of a truncated source silently.
    if let Some(buf) = fence {
        let body = buf.trim().to_string();
        if !body.is_empty() {
            push_text(&mut text, &body);
            latex_math.push(body);
        }
    }

    Passage {
        locator,
        text,
        latex_math,
    }
}

/// Parse MinerU's Markdown export into section and statement passages.
///
/// A section heading (see [`markdown_section_number`]) opens a section running
/// to the next heading. Every statement marker inside it (see
/// [`markdown_statement_marker`]) additionally opens a statement passage,
/// running to the proof that discharges it, or to the next marker where there
/// is no proof. The section passage is emitted before its statements, so
/// resolving a section locator returns the section.
pub fn load_passages_markdown(path: &Path) -> anyhow::Result<Vec<Passage>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading markdown source {}", path.display()))?;
    Ok(parse_markdown(&raw))
}

fn parse_markdown(raw: &str) -> Vec<Passage> {
    let lines: Vec<&str> = raw.lines().collect();

    // Prefer the strict form. Fall back to the single-number form only when the
    // document's own numbering vouches for it by increasing throughout, which a
    // numbered list interleaved with real sections never does. A source whose
    // strict headings already cover it is left exactly as it was.
    let strict: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| markdown_section_number(line).map(|n| (i, n)))
        .collect();

    let heads: Vec<(usize, String)> = if strict.is_empty() {
        let loose: Vec<(usize, String)> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| markdown_section_number_loose(line).map(|n| (i, n)))
            .collect();
        let numbers: Vec<String> = loose.iter().map(|(_, n)| n.clone()).collect();
        if loose.len() > 1 && strictly_increasing(&numbers) {
            loose
        } else {
            strict
        }
    } else {
        strict
    };

    let mut passages = Vec::new();

    for (k, (start, number)) in heads.iter().enumerate() {
        let end = heads.get(k + 1).map_or(lines.len(), |(i, _)| *i);
        let body = &lines[start + 1..end];

        passages.push(build_passage(number.clone(), body));

        let marks: Vec<(usize, &'static str, String)> = body
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                markdown_statement_marker(line).map(|(abbrev, num)| (i, abbrev, num))
            })
            .collect();

        for (m, (mark_start, abbrev, num)) in marks.iter().enumerate() {
            let next = marks.get(m + 1).map_or(body.len(), |(i, _, _)| *i);
            let stop = body[mark_start + 1..next]
                .iter()
                .position(|line| is_proof_line(line))
                .map_or(next, |offset| mark_start + 1 + offset);
            passages.push(build_passage(
                format!("{number} {abbrev} {num}"),
                &body[*mark_start..stop],
            ));
        }
    }

    passages
}

#[cfg(test)]
mod markdown_tests {
    use super::*;

    /// Written in the line forms MinerU's Markdown export actually produces,
    /// with invented mathematical content. The real sources are copyrighted
    /// textbooks and cannot be committed, so the fixture reproduces the
    /// format and none of the prose.
    const FIXTURE: &str = "\
2. SOME CHAPTER HEADING

1. Laplace's equation
2. Helmholtz's equation

4.2.1. Fictional regularity.

We assume throughout that the fictional operator is bounded.

$$
\\|u\\|_{X} \\le C \\|f\\|_{Y}\\tag{1}
$$

THEOREM 1 (Fictional bound). Assume the coefficients are smooth and f lies in Y.

Then the estimate holds with C depending only on the domain.

Proof. 1. We first reduce to the model case.

2. The general case follows by covering.

THEOREM 2 (Second fictional bound). Assume in addition that the domain is convex.

Proof. Immediate from Theorem 1.

DEFINITION 3. A fictional pair is admissible when it satisfies (1).

4.2.2. Another fictional section.

Nothing of substance here.
";

    fn passage<'a>(passages: &'a [Passage], locator: &str) -> &'a Passage {
        passages
            .iter()
            .find(|p| p.locator == locator)
            .unwrap_or_else(|| panic!("no passage with locator {locator:?}"))
    }

    #[test]
    fn section_headings_are_recognised_and_numbered_lists_are_not() {
        assert_eq!(
            markdown_section_number("4.2.1. Fictional regularity."),
            Some("4.2.1".to_string())
        );
        assert_eq!(
            markdown_section_number("6.1. DEFINITIONS"),
            Some("6.1".to_string())
        );
        // A single leading number is a list item, not a heading.
        assert_eq!(markdown_section_number("1. Laplace's equation"), None);
        // Chapter-level headings are out of reach for the same reason.
        assert_eq!(markdown_section_number("2. SOME CHAPTER HEADING"), None);
        // A number with no title is not a heading.
        assert_eq!(markdown_section_number("4.2.1."), None);
    }

    #[test]
    fn statement_markers_parse_with_their_number() {
        assert_eq!(
            markdown_statement_marker("THEOREM 1 (Fictional bound). Assume"),
            Some(("Thm", "1".to_string()))
        );
        assert_eq!(
            markdown_statement_marker("DEFINITION 3. A fictional pair"),
            Some(("Def", "3".to_string()))
        );
        // Unnumbered statements stay inside their section passage.
        assert_eq!(markdown_statement_marker("THEOREM (Unnumbered)."), None);
        // A longer word merely starting with a marker is not a marker.
        assert_eq!(markdown_statement_marker("THEOREMS 1 and 2 below"), None);
    }

    #[test]
    fn proof_lines_close_a_statement() {
        assert!(is_proof_line("Proof. 1. We first reduce"));
        assert!(is_proof_line("Proof:"));
        assert!(is_proof_line("Proof"));
        assert!(!is_proof_line("Proofs of both are given below"));
    }

    #[test]
    fn a_section_and_each_statement_inside_it_are_addressable() {
        let passages = parse_markdown(FIXTURE);
        let locators: Vec<&str> = passages.iter().map(|p| p.locator.as_str()).collect();
        assert_eq!(
            locators,
            vec![
                "4.2.1",
                "4.2.1 Thm 1",
                "4.2.1 Thm 2",
                "4.2.1 Def 3",
                "4.2.2",
            ]
        );
    }

    #[test]
    fn the_section_passage_precedes_its_statements() {
        let passages = parse_markdown(FIXTURE);
        let section = passages.iter().position(|p| p.locator == "4.2.1").unwrap();
        let statement = passages
            .iter()
            .position(|p| p.locator == "4.2.1 Thm 1")
            .unwrap();
        assert!(section < statement);
    }

    /// The granularity fix: a statement passage carries its own theorem and
    /// not the ones beside it.
    #[test]
    fn a_statement_excludes_its_proof_and_its_neighbours() {
        let passages = parse_markdown(FIXTURE);
        let thm1 = passage(&passages, "4.2.1 Thm 1");

        assert!(thm1.text.contains("Fictional bound"));
        assert!(thm1.text.contains("C depending only on the domain"));
        // The proof is not part of the statement.
        assert!(!thm1.text.contains("reduce to the model case"));
        // Nor is the next theorem.
        assert!(!thm1.text.contains("Second fictional bound"));
        // Nor is the section preamble.
        assert!(!thm1.text.contains("bounded"));
    }

    #[test]
    fn a_statement_with_no_proof_runs_to_the_next_marker() {
        let passages = parse_markdown(FIXTURE);
        let def = passage(&passages, "4.2.1 Def 3");
        assert!(def.text.contains("admissible"));
        assert!(!def.text.contains("Another fictional section"));
    }

    #[test]
    fn the_section_passage_carries_everything_under_its_heading() {
        let passages = parse_markdown(FIXTURE);
        let section = passage(&passages, "4.2.1");
        assert!(section.text.contains("bounded"));
        assert!(section.text.contains("Fictional bound"));
        assert!(section.text.contains("Second fictional bound"));
        assert!(section.text.contains("reduce to the model case"));
        // and stops at the next heading
        assert!(!section.text.contains("Nothing of substance"));
    }

    /// The measured reason statement granularity matters, in miniature: the
    /// section operand is several times the statement operand.
    #[test]
    fn a_statement_is_a_much_smaller_operand_than_its_section() {
        let passages = parse_markdown(FIXTURE);
        let section = passage(&passages, "4.2.1").text.len();
        let statement = passage(&passages, "4.2.1 Thm 1").text.len();
        assert!(
            statement * 3 < section,
            "statement {statement} vs section {section}"
        );
    }

    #[test]
    fn display_equations_are_captured_as_latex_and_kept_in_the_text() {
        let passages = parse_markdown(FIXTURE);
        let section = passage(&passages, "4.2.1");
        assert_eq!(section.latex_math.len(), 1);
        assert!(section.latex_math[0].contains("\\le C \\|f\\|_{Y}"));
        assert!(section.text.contains("\\le C \\|f\\|_{Y}"));
    }

    #[test]
    fn format_dispatches_on_extension() {
        assert_eq!(
            SourceFormat::of_path(Path::new("a/b.md")),
            SourceFormat::Markdown
        );
        assert_eq!(
            SourceFormat::of_path(Path::new("a/b.MD")),
            SourceFormat::Markdown
        );
        // Anything else stays the content-list loader, so manifests written
        // before this loader existed keep working.
        assert_eq!(
            SourceFormat::of_path(Path::new("a/b.content_list.json")),
            SourceFormat::ContentList
        );
        assert_eq!(SourceFormat::ContentList.as_str(), "content_list");
        assert_eq!(SourceFormat::Markdown.as_str(), "markdown");
    }

    /// An arXiv paper, whose sections carry one number and no trailing dot.
    const MARKDOWN_PAPER: &str = "\
1 Introduction

Prose that belongs to the introduction.

2 Upper bound for the Infimum

THEOREM 1 (Fictional bound). The infimum is below the double sphere.

Proof. Immediate.

3 Convergence

Closing prose.
";

    #[test]
    fn paper_sections_resolve_when_their_numbering_increases() {
        let passages = parse_markdown(MARKDOWN_PAPER);
        let locators: Vec<&str> = passages.iter().map(|p| p.locator.as_str()).collect();
        assert!(locators.contains(&"1"), "got {locators:?}");
        assert!(locators.contains(&"2"), "got {locators:?}");
        assert!(locators.contains(&"3"), "got {locators:?}");
        // The statement inside a paper section is addressable too, which is the
        // granularity a warrant needs.
        assert!(locators.contains(&"2 Thm 1"), "got {locators:?}");
        assert!(passage(&passages, "2 Thm 1")
            .text
            .contains("below the double sphere"));
    }

    #[test]
    fn a_textbook_is_parsed_exactly_as_before() {
        // The strict rule finds headings here, so the fallback never runs and
        // the numbered list inside the proof stays out of the heading set.
        let passages = parse_markdown(FIXTURE);
        let sections: Vec<&str> = passages
            .iter()
            .map(|p| p.locator.as_str())
            .filter(|l| !l.contains(' '))
            .collect();
        assert_eq!(sections, vec!["4.2.1", "4.2.2"]);
    }

    #[test]
    fn a_numbered_list_that_restarts_opens_no_sections() {
        let listing = "1. First item\n\n2. Second item\n\n1. Restarting item\n\nProse.\n";
        let passages = parse_markdown(listing);
        assert!(passages.is_empty(), "got {passages:?}");
    }

    #[test]
    fn an_increasing_enumeration_is_still_not_a_section_list() {
        // The case that defeated the first version of this fallback: an
        // enumeration numbering from one, increasing, and so indistinguishable
        // from a paper's sections by monotonicity alone. The trailing dot
        // rejects it.
        let listing = "1. First item\n\n2. Second item\n\n3. Third item\n\nProse.\n";
        assert!(parse_markdown(listing).is_empty());
    }

    #[test]
    fn a_paper_mixing_sections_and_an_enumeration_keeps_only_the_sections() {
        // schygulla-2011-wil-min-pre in miniature: sections and an enumerated
        // list both numbering from one, in the same document.
        let mixed = "1 Introduction\n\nProse.\n\n2 Upper bound\n\n\
1. The sets are discs.\n\n2. The map is smooth.\n\n3 Convergence\n\nMore.\n";
        let passages = parse_markdown(mixed);
        let locators: Vec<&str> = passages.iter().map(|p| p.locator.as_str()).collect();
        assert_eq!(locators, vec!["1", "2", "3"]);
    }

    #[test]
    fn the_loose_form_takes_a_single_number_with_or_without_a_dot() {
        assert_eq!(
            markdown_section_number_loose("2 Upper bound for the Infimum"),
            Some("2".to_string())
        );
        // The trailing dot marks an enumerator, so the dotted forms are
        // rejected here however deep they are. That is the whole discriminator.
        assert_eq!(markdown_section_number_loose("2. Upper bound"), None);
        assert_eq!(
            markdown_section_number_loose("1. The sets are topological discs."),
            None
        );
        assert_eq!(
            markdown_section_number_loose("4.2.1. Fictional regularity."),
            None
        );
        // A bare number with no title is not a heading, nor is a lowercase
        // continuation carrying a leading numeral.
        assert_eq!(markdown_section_number_loose("2"), None);
        assert_eq!(markdown_section_number_loose("Introduction"), None);
        assert_eq!(
            markdown_section_number_loose("2 of the sets are discs"),
            None
        );
    }

    #[test]
    fn statement_markers_are_taken_in_caps_and_in_title_case() {
        // The textbook form.
        assert_eq!(
            markdown_statement_marker("THEOREM 1 (Interior bound)."),
            Some(("Thm", "1".to_string()))
        );
        // The paper form, numbered by section.
        assert_eq!(
            markdown_statement_marker("Lemma 2.1 The function is decreasing"),
            Some(("Lem", "2.1".to_string()))
        );
        assert_eq!(
            markdown_statement_marker("Theorem 1.1. Existence holds."),
            Some(("Thm", "1.1".to_string()))
        );
        // A lowercase marker is prose resuming, not a statement opening.
        assert_eq!(markdown_statement_marker("theorem 2.1 gives this"), None);
        // A marker with no number stays reachable through its section only.
        assert_eq!(markdown_statement_marker("Lemma (unnumbered)"), None);
    }

    #[test]
    fn a_paper_statement_is_a_far_smaller_operand_than_its_section() {
        let mixed = "1 Introduction\n\nProse.\n\n2 Upper bound\n\n\
Some framing prose that belongs to the section and not to the lemma.\n\n\
Lemma 2.1 The function is decreasing and bounded above.\n\n\
Proof. Immediate.\n\n3 Convergence\n\nMore.\n";
        let passages = parse_markdown(mixed);
        let section = passage(&passages, "2").text.len();
        let statement = passage(&passages, "2 Lem 2.1").text.len();
        assert!(
            statement < section,
            "statement {statement} should be smaller than section {section}"
        );
        assert!(passage(&passages, "2 Lem 2.1")
            .text
            .contains("decreasing and bounded"));
    }

    #[test]
    fn monotonicity_is_what_separates_sections_from_a_list() {
        let sections: Vec<String> = ["6.3.1", "6.3.2", "6.4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(strictly_increasing(&sections));
        let interleaved: Vec<String> = ["6.3.1", "1", "6.3.2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(!strictly_increasing(&interleaved));
    }
}
