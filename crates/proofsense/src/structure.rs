//! Structural comparison of a passage against a declaration, over the
//! function spaces each names.
//!
//! # What this is for, and what it is not
//!
//! The judge is handed two pieces of English. Both sides also carry a formal
//! object that the judge never sees: the passage's display equations
//! ([`crate::ingest::Passage::latex_math`]) and the declaration's
//! pretty-printed Lean type ([`crate::lean::LeanDeclInfo::type_pp`]). The
//! function spaces named in each are extractable by rule, and a space present
//! on one side and absent on the other is a difference worth reading.
//!
//! This is **advisory and never sets a rung**. Structural comparison of
//! theorem statements is a measured technique with modest reliability:
//! ASSESS/TransTED (arXiv 2509.22246) reports 70.16% accuracy at Cohen's kappa
//! 0.35 comparing *formal* statements to *formal* statements by operator-tree
//! edit distance. Comparing LaTeX against Lean is the harder direction of a
//! technique that only reaches fair agreement in the easier one, so the output
//! here is a difference for a human to read, never a verdict. The rung comes
//! from [`crate::verdict::classify`] and nothing in this module feeds it.
//!
//! # Polarity is invisible here, and that is the sharp limit
//!
//! Measured against the real transcription on 2026-07-27, this check does
//! **not** isolate the divergence the 2026-07-22 warrant audit found on
//! `interior_H2_estimate`, and the reason is worth stating.
//!
//! Evans §6.3.1 Theorem 1 mentions `H¹₀(U)` exactly once, in its Remark (i):
//! "Note carefully that we do not require `u ∈ H¹₀(U)`". The declaration
//! quantifies `u` over `H01 Ω`. So the one place the source names that space
//! is to *deny* it, the declaration *requires* it, and a set intersection
//! reports the two sides as agreeing on `H1_0`.
//!
//! A set of names carries no polarity, no quantifier and no direction. This is
//! the concrete form of why the output is advisory: it can say two statements
//! draw on different vocabulary, and it can never say what either asserts.
//!
//! # What it does surface
//!
//! Against the same pair it reports five spaces the passage names and the
//! declaration never does, among them `H1` and `H2`. That is a real
//! observation: Evans states the conclusion as `u ∈ H²_loc(U)` while the
//! declaration expresses it through `HasWeakDerivOn` and norm bounds, naming
//! no `H²` at all. A reader comparing the two benefits from being told.
//!
//! # The two sides
//!
//! LaTeX is parsed by rule: a family letter in `H`, `L`, `C`, `W` carrying a
//! superscript or subscript. Whitespace is stripped first, since MinerU's
//! transcription spaces these out (`H ^ { 2 }`).
//!
//! Lean names are project-specific (`H01`, `L2D`, `IsC1Coeff`), so they are
//! mapped through a table the manifest supplies rather than a table compiled
//! in. This keeps proofsense free of any one development's vocabulary and puts
//! the mapping where its author can be held to it, in the same spirit as the
//! verbaliser's small trusted reading table.

use std::collections::{BTreeMap, BTreeSet};

/// Maps a Lean identifier to the canonical space name it denotes, e.g.
/// `"H01" -> "H1_0"`. Supplied by the manifest.
pub type SpaceMap = BTreeMap<String, String>;

/// Function spaces named on each side, and how they differ.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SpaceComparison {
    /// Named on both sides.
    pub shared: Vec<String>,
    /// Named by the passage and absent from the declaration.
    pub passage_only: Vec<String>,
    /// Named by the declaration and absent from the passage.
    pub decl_only: Vec<String>,
}

impl SpaceComparison {
    /// Whether the two sides name exactly the same spaces. A difference is a
    /// prompt to read, never a defect on its own: a declaration may
    /// legitimately work in a space the passage leaves implicit.
    pub fn agrees(&self) -> bool {
        self.passage_only.is_empty() && self.decl_only.is_empty()
    }
}

/// Family letters that open a function-space name.
const FAMILIES: [char; 4] = ['H', 'L', 'C', 'W'];

/// Advance past any whitespace, which the transcription sprinkles through
/// these names (`H ^ { 2 }`).
fn skip_space(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

/// Read a `{...}` group, or a single following token, returning its content
/// and the index just past it.
fn read_group(chars: &[char], start: usize) -> (String, usize) {
    let start = skip_space(chars, start);
    if start >= chars.len() {
        return (String::new(), start);
    }
    if chars[start] == '{' {
        let mut depth = 0usize;
        let mut out = String::new();
        for (offset, &c) in chars[start..].iter().enumerate() {
            match c {
                '{' => {
                    depth += 1;
                    if depth == 1 {
                        continue;
                    }
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (out, start + offset + 1);
                    }
                }
                _ => {}
            }
            out.push(c);
        }
        return (out, chars.len());
    }
    // A backslash command such as `\infty`, otherwise one character.
    if chars[start] == '\\' {
        let mut out = String::from("\\");
        let mut i = start + 1;
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            out.push(chars[i]);
            i += 1;
        }
        return (out, i);
    }
    (chars[start].to_string(), start + 1)
}

/// Formatting macros that wrap an index without contributing to its name, so
/// `H^{2}_{\mathrm{loc}}` reads as `H2_loc` rather than `H2_mathrmloc`.
const FORMATTING_MACROS: [&str; 5] = [
    "\\mathrm",
    "\\operatorname",
    "\\text",
    "\\mathbb",
    "\\mathcal",
];

/// Canonicalise an exponent or subscript: drop formatting macros, shorten
/// `infty` to `inf`, and keep what is alphanumeric.
fn canonical_index(raw: &str) -> String {
    let mut work = raw.to_string();
    for macro_name in FORMATTING_MACROS {
        work = work.replace(macro_name, "");
    }
    work.replace("\\infty", "inf")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Assemble the canonical name for a family with its indices, e.g.
/// `H` with superscript `1` and subscript `0` becomes `H1_0`.
fn canonical_space(family: char, sup: Option<String>, sub: Option<String>) -> Option<String> {
    let sup = sup.map(|s| canonical_index(&s)).filter(|s| !s.is_empty());
    let sub = sub.map(|s| canonical_index(&s)).filter(|s| !s.is_empty());
    match (sup, sub) {
        (None, None) => None,
        (Some(p), None) => Some(format!("{family}{p}")),
        (None, Some(b)) => Some(format!("{family}_{b}")),
        (Some(p), Some(b)) => Some(format!("{family}{p}_{b}")),
    }
}

/// Every function space named in a LaTeX or transcribed-prose fragment.
///
/// Whitespace inside a name is skipped, so MinerU's `H ^ { 2 }` reads the same
/// as `H^2`, while whitespace *before* the family letter still separates it
/// from what precedes. That distinction matters: stripping whitespace globally
/// runs `\in` into the following letter, so `f \in L^{2}` would lose its `L`
/// to the `n`.
///
/// A family letter preceded by an alphanumeric character or a backslash opens
/// nothing, so a variable named `aH` and the macro `\Lambda` are both skipped.
pub fn spaces_in_latex(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = BTreeSet::new();

    for i in 0..chars.len() {
        let family = chars[i];
        if !FAMILIES.contains(&family) {
            continue;
        }
        if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '\\') {
            continue;
        }

        let mut j = skip_space(&chars, i + 1);
        let (mut sup, mut sub) = (None, None);
        // A family may carry a superscript, a subscript, or both in either
        // order: `H_0^1` and `H^1_0` name the same space.
        for _ in 0..2 {
            if j >= chars.len() || (chars[j] != '^' && chars[j] != '_') {
                break;
            }
            let kind = chars[j];
            let (value, next) = read_group(&chars, j + 1);
            if value.trim().is_empty() {
                break;
            }
            let value: String = value.chars().filter(|c| !c.is_whitespace()).collect();
            if kind == '^' {
                sup = Some(value);
            } else {
                sub = Some(value);
            }
            j = skip_space(&chars, next);
        }

        if let Some(name) = canonical_space(family, sup, sub) {
            found.insert(name);
        }
    }

    found
}

/// Every function space a Lean type names, resolved through the manifest's
/// map. An identifier counts only as a whole token, so `H01` in the map does
/// not match `H01Extended` in the type.
pub fn spaces_in_lean(type_pp: &str, map: &SpaceMap) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (identifier, space) in map {
        if contains_token(type_pp, identifier) {
            found.insert(space.clone());
        }
    }
    found
}

/// Whether `haystack` contains `needle` as a whole identifier.
///
/// A namespace dot separates rather than extends, so the map key `H01` matches
/// `EllipticPdes.Sobolev.H01` and a manifest needs no namespace prefix. A
/// letter or digit on either side does extend, so `H01Extended` and `myH01`
/// both fail to match.
fn contains_token(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let mut from = 0usize;
    while let Some(offset) = haystack[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = !haystack[..start].chars().next_back().is_some_and(is_ident);
        let after_ok = !haystack[end..].chars().next().is_some_and(is_ident);
        if before_ok && after_ok {
            return true;
        }
        from = start + needle.len();
        if from >= haystack.len() {
            break;
        }
    }
    false
}

/// Compare the spaces a passage names against those its declaration names.
pub fn compare(passage_text: &str, type_pp: &str, map: &SpaceMap) -> SpaceComparison {
    let passage = spaces_in_latex(passage_text);
    let decl = spaces_in_lean(type_pp, map);

    SpaceComparison {
        shared: passage.intersection(&decl).cloned().collect(),
        passage_only: passage.difference(&decl).cloned().collect(),
        decl_only: decl.difference(&passage).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> SpaceMap {
        [
            ("H01", "H1_0"),
            ("L2D", "L2"),
            ("IsC1Coeff", "C1"),
            ("H1amb", "H1"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn a_superscript_names_a_space() {
        assert_eq!(spaces_in_latex("f \\in L^{2}(U)"), set(["L2"]));
        assert_eq!(spaces_in_latex("u \\in H^2(V)"), set(["H2"]));
    }

    /// MinerU spaces out the indices, so whitespace cannot be significant.
    #[test]
    fn transcription_whitespace_is_ignored() {
        assert_eq!(spaces_in_latex("H ^ { 2 }"), set(["H2"]));
        assert_eq!(spaces_in_latex("C ^ { 1 } ( U )"), set(["C1"]));
    }

    #[test]
    fn subscript_and_superscript_combine_in_either_order() {
        assert_eq!(spaces_in_latex("H_{0}^{1}"), set(["H1_0"]));
        assert_eq!(spaces_in_latex("H^{1}_{0}"), set(["H1_0"]));
    }

    #[test]
    fn infinity_is_shortened() {
        assert_eq!(spaces_in_latex("C^{\\infty}(U)"), set(["Cinf"]));
        assert_eq!(spaces_in_latex("L^\\infty"), set(["Linf"]));
    }

    #[test]
    fn a_family_letter_inside_a_word_opens_nothing() {
        // A variable subscripted, not a space.
        assert!(spaces_in_latex("aH^2").is_empty());
        // A macro whose name begins with a family letter.
        assert!(spaces_in_latex("\\Lambda^2").is_empty());
    }

    /// The guard has to survive a LaTeX command running into the family
    /// letter, which is what `\in L^{2}` looks like once whitespace stops
    /// separating them.
    #[test]
    fn a_command_before_the_family_letter_does_not_suppress_it() {
        assert_eq!(spaces_in_latex("f \\in L^{2}(U)"), set(["L2"]));
        assert_eq!(spaces_in_latex("u \\in H^{1}(U)"), set(["H1"]));
    }

    #[test]
    fn a_bare_family_letter_names_nothing() {
        assert!(spaces_in_latex("the constant C depending on V").is_empty());
    }

    #[test]
    fn several_spaces_in_one_statement() {
        let text = "Assume a^{ij} \\in C^{1}(U), f \\in L^{2}(U) and u \\in H^{1}(U); \
                    then u \\in H^{2}_{loc}(U)";
        assert_eq!(spaces_in_latex(text), set(["C1", "L2", "H1", "H2_loc"]));
    }

    #[test]
    fn lean_identifiers_resolve_through_the_manifest_map() {
        let ty = "∀ (u : ↥(EllipticPdes.Sobolev.H01 Ω)) (f : EllipticPdes.Sobolev.L2D Ω), True";
        assert_eq!(spaces_in_lean(ty, &map()), set(["H1_0", "L2"]));
    }

    #[test]
    fn a_mapped_identifier_matches_only_as_a_whole_token() {
        assert_eq!(spaces_in_lean("H01Extended Ω", &map()), set([]));
        assert_eq!(spaces_in_lean("myH01 Ω", &map()), set([]));
        assert_eq!(spaces_in_lean("Sobolev.H01 Ω", &map()), set(["H1_0"]));
    }

    /// The 2026-07-22 audit's finding, reproduced by rule: Evans quantifies
    /// over `H^1` and the declaration over `H01`, and neither side names the
    /// other's space.
    #[test]
    fn the_interior_h2_divergence_shows_up_as_a_difference() {
        let passage = "Assume a^{ij} \\in C^{1}(U), f \\in L^{2}(U), and suppose \
                       u \\in H^{1}(U) is a weak solution. Then u \\in H^{2}_{loc}(U).";
        let ty = "∀ (u : ↥(EllipticPdes.Sobolev.H01 Ω)) (f : EllipticPdes.Sobolev.L2D Ω), True";

        let c = compare(passage, ty, &map());
        assert!(!c.agrees());
        assert!(c.passage_only.contains(&"H1".to_string()));
        assert!(c.decl_only.contains(&"H1_0".to_string()));
        assert!(c.shared.contains(&"L2".to_string()));
    }

    #[test]
    fn agreement_is_reported_when_both_sides_name_the_same_spaces() {
        let passage = "for u \\in H^{1}_{0}(U) and f \\in L^{2}(U)";
        let ty = "∀ (u : ↥(H01 Ω)) (f : L2D Ω), True";
        let c = compare(passage, ty, &map());
        assert!(c.agrees(), "{c:?}");
        assert_eq!(c.shared, vec!["H1_0", "L2"]);
    }

    #[test]
    fn an_empty_map_names_nothing_on_the_lean_side() {
        let c = compare(
            "u \\in H^{1}(U)",
            "∀ (u : ↥(H01 Ω)), True",
            &SpaceMap::new(),
        );
        assert_eq!(c.decl_only, Vec::<String>::new());
        assert_eq!(c.passage_only, vec!["H1"]);
    }

    fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
}
