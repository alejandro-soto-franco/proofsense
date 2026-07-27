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
//! 2. Canonicalise each whitespace-separated token: a leading statement word
//!    (`Theorem`, `Thm`, `Lemma`, ...) becomes its canonical abbreviation,
//!    and a separator dot after it is dropped.
//! 3. Lowercase, and strip all whitespace.
//!
//! Two locators match iff their normalised tokens are equal.
//!
//! Examples: `"§6.3.1"` -> `"6.3.1"`; `"§6.3.1 Thm 1"` -> `"6.3.1thm1"`;
//! `"Thm 8.8"` -> `"thm8.8"`; `"Theorem  8.8"` -> `"thm8.8"`.
//!
//! # Why the marker is kept
//!
//! Stripping the marker instead, as this module did before statement
//! granularity, collapses `"Thm 8.8"` and `"§8.8"` onto the same token. That
//! is harmless for a source numbering its theorems inside sections, as Evans
//! does, and wrong for Gilbarg and Trudinger, whose theorems are numbered
//! `8.8` at chapter level alongside sections `8.1` to `8.12`: a warrant citing
//! `Thm 8.8` resolves silently to section 8.8 and the run then reports a
//! verdict about the wrong passage.

use crate::ingest::Passage;

/// Statement words and the canonical abbreviation each normalises to. Longer
/// spellings precede their prefixes so the longest match wins.
const MARKER_WORDS: [(&str, &str); 10] = [
    ("theorem", "thm"),
    ("thm", "thm"),
    ("lemma", "lem"),
    ("lem", "lem"),
    ("corollary", "cor"),
    ("cor", "cor"),
    ("definition", "def"),
    ("defn", "def"),
    ("def", "def"),
    ("remark", "rmk"),
];

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

/// Canonicalise one token: rewrite a leading statement word to its
/// abbreviation and drop a separator dot after it. `"Theorem"` and `"Thm."`
/// both become `"thm"`, and `"THM8.8"` becomes `"thm8.8"`.
fn canonical_token(token: &str) -> String {
    for (word, abbrev) in MARKER_WORDS {
        if let Some(rest) = strip_prefix_word(token, word) {
            let rest = rest.strip_prefix('.').unwrap_or(rest);
            return format!("{abbrev}{}", rest.to_lowercase());
        }
    }
    token.to_lowercase()
}

/// Normalise a locator string per the module-level rule.
fn normalise(locator: &str) -> String {
    let without_section_mark: String = locator.chars().filter(|&c| c != '§').collect();
    without_section_mark
        .split_whitespace()
        .map(canonical_token)
        .collect()
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
    fn normalise_canonicalises_theorem_spellings() {
        assert_eq!(normalise("Thm 8.8"), "thm8.8");
        assert_eq!(normalise("Theorem  8.8"), "thm8.8");
        assert_eq!(normalise("theorem 8.8"), "thm8.8");
        assert_eq!(normalise("THM8.8"), "thm8.8");
        assert_eq!(normalise("Thm. 8.8"), "thm8.8");
    }

    #[test]
    fn normalise_canonicalises_every_statement_word() {
        assert_eq!(normalise("Lemma 3"), "lem3");
        assert_eq!(normalise("Corollary 3"), "cor3");
        assert_eq!(normalise("Definition 3"), "def3");
        assert_eq!(normalise("Remark 3"), "rmk3");
    }

    /// The collision this module carried before statement granularity: a
    /// theorem numbered at chapter level and the section of the same number
    /// must not resolve to the same passage.
    #[test]
    fn normalise_separates_a_theorem_from_the_section_of_the_same_number() {
        assert_ne!(normalise("Thm 8.8"), normalise("§8.8"));
        assert_eq!(normalise("§8.8"), "8.8");
    }

    #[test]
    fn normalise_separates_a_statement_from_its_containing_section() {
        assert_eq!(normalise("§6.3.1 Thm 1"), "6.3.1thm1");
        assert_ne!(normalise("§6.3.1 Thm 1"), normalise("§6.3.1"));
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

    #[test]
    fn resolve_picks_the_statement_over_the_section_that_contains_it() {
        let passages = [
            Passage {
                locator: "6.3.1".to_string(),
                text: "three theorems".to_string(),
                latex_math: Vec::new(),
            },
            Passage {
                locator: "6.3.1 Thm 1".to_string(),
                text: "one theorem".to_string(),
                latex_math: Vec::new(),
            },
        ];
        assert_eq!(resolve(&passages, "§6.3.1").unwrap().text, "three theorems");
        assert_eq!(
            resolve(&passages, "§6.3.1 Thm 1").unwrap().text,
            "one theorem"
        );
    }
}
