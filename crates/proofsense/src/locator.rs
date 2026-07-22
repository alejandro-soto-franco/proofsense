//! Deterministic structural locator resolution: map a citation locator
//! string (e.g. `"§6.3.1"`, `"6.3.1"`, `"Thm 8.8"`, `"Theorem 8.8"`) to the
//! [`Passage`] whose structural locator names the same section/theorem
//! number. This is a pure string-equality rule after normalisation, with no
//! embeddings, no semantic/fuzzy matching, so the result is reproducible
//! and auditable.
//!
//! # Normalisation rule
//!
//! Both the query locator and each passage's `locator` are normalised
//! before comparison, by applying, in order:
//! 1. Strip every `§` section-mark character.
//! 2. Strip a leading `Thm` or `Theorem` word (case-insensitive).
//! 3. Strip all whitespace (leading, trailing, and internal).
//!
//! The result is a bare token (typically a dotted section/theorem number,
//! e.g. `"6.3.1"` or `"8.8"`). Two locators match iff their normalised
//! tokens are equal.
//!
//! Examples: `"§6.3.1"` -> `"6.3.1"`; `"6.3.1"` -> `"6.3.1"`;
//! `"Thm 8.8"` -> `"8.8"`; `"Theorem  8.8"` -> `"8.8"`.

use crate::ingest::Passage;

/// If `s` starts with `word` (case-insensitive, ASCII), return the
/// remainder of `s` after that word; otherwise `None`.
fn strip_prefix_word<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    if s.len() < word.len() || !s.is_char_boundary(word.len()) {
        return None;
    }
    if s[..word.len()].eq_ignore_ascii_case(word) {
        Some(&s[word.len()..])
    } else {
        None
    }
}

/// Normalise a locator string per the module-level rule: strip `§`, strip a
/// leading `Thm`/`Theorem` word, strip all whitespace.
fn normalise(locator: &str) -> String {
    let without_section_mark: String = locator.chars().filter(|&c| c != '§').collect();
    let trimmed = without_section_mark.trim();

    let body = strip_prefix_word(trimmed, "theorem")
        .or_else(|| strip_prefix_word(trimmed, "thm"))
        .unwrap_or(trimmed);

    body.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Resolve a citation `locator` (e.g. `"§6.3.1"`) to the first passage in
/// `passages` whose locator normalises to the same token as `locator`
/// (see the module-level normalisation rule). Iterates `passages` in
/// order and returns the first structural match, or `None` if none match.
/// Deterministic: given the same inputs, always returns the same result.
pub fn resolve<'a>(passages: &'a [Passage], locator: &str) -> Option<&'a Passage> {
    let target = normalise(locator);
    passages.iter().find(|p| normalise(&p.locator) == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_strips_section_mark_and_whitespace() {
        assert_eq!(normalise("§6.3.1"), "6.3.1");
        assert_eq!(normalise("6.3.1"), "6.3.1");
        assert_eq!(normalise("  § 6.3.1  "), "6.3.1");
    }

    #[test]
    fn normalise_strips_theorem_prefix_case_insensitively() {
        assert_eq!(normalise("Thm 8.8"), "8.8");
        assert_eq!(normalise("Theorem  8.8"), "8.8");
        assert_eq!(normalise("theorem 8.8"), "8.8");
        assert_eq!(normalise("THM8.8"), "8.8");
    }

    #[test]
    fn resolve_returns_none_when_no_passage_matches() {
        let passages = [Passage {
            locator: "6.3.1".to_string(),
            text: "x".to_string(),
            latex_math: Vec::new(),
        }];
        assert!(resolve(&passages, "§9.9.9").is_none());
    }
}
