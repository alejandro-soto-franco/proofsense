//! Literature ingest: turn a MinerU `content_list.json` into section-locator
//! addressable passages.
//!
//! MinerU's `content_list.json` (pipeline backend, legacy flat format) is a
//! JSON array of block objects. Relevant fields (confirmed against the MinerU
//! docs, `docs/en/reference/output_files.md`):
//!   - `type`: "text" | "equation" | "image" | "table" | ...
//!   - `text`: the block's textual content (for text/equation blocks). For
//!     equation blocks the LaTeX lives here, wrapped in `$$...$$`.
//!   - `text_level`: present (>= 1) only on heading blocks; absent or 0 on
//!     ordinary body text. A heading is `type:"text"` with `text_level` set.
//!   - `page_idx`, `bbox`, `text_format`, `img_path`, ...: ignored here.
//!
//! Unknown fields are tolerated (serde ignores them).

use anyhow::Context;
use serde::Deserialize;
use std::path::Path;

/// A section-scoped passage: the text under one heading, plus any display
/// equations that appeared inside it (as raw LaTeX).
#[derive(Debug, Clone)]
pub struct Passage {
    /// Structural locator for the passage, e.g. "6.3.1".
    pub locator: String,
    /// Concatenated body text of the section (prose + inline equation LaTeX).
    pub text: String,
    /// Display-equation LaTeX blocks appearing in the section, in order.
    pub latex_math: Vec<String>,
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
