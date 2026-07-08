//! Verdict types: the trust-rung lattice, the entailment judgement, the
//! per-warrant [`Verdict`] record, and human-readable report rendering.
//!
//! See the SPEC's "Trust model — the obligation lattice" (§4): every warrant
//! is labelled with exactly how strong its check is. `TrustRung::Entailed`
//! is evidence-grade (default LLM-NLI against a faithful, deterministically
//! generated operand) — never conflate it with a soundness proof.
//! `TrustRung::EntailedFormal` is reserved for the (future) round-trip
//! re-autoformalisation + in-kernel discharge; it is strictly stronger and
//! must never be assigned by the evidence-grade path.

use std::fmt;

/// A warrant's position in the obligation lattice, from weakest to
/// strongest. Mirrors SPEC §4 exactly:
///
/// - [`TrustRung::Bare`] — a citation only; no target passage resolved.
/// - [`TrustRung::Targeted`] — the locator resolved to a specific
///   transcribed passage (a concrete target), but no correspondence check
///   has been run yet.
/// - [`TrustRung::Entailed`] — the passage was entailment-checked against
///   the machine English and passed. **Evidence-grade**: the default is
///   LLM-NLI with the faithful operand pinned. This is evidence, not proof.
/// - [`TrustRung::EntailedFormal`] — the passage was re-autoformalised and
///   discharged equivalent to the decl in-kernel (**formal-evidence-grade**:
///   a round-trip check, Increment 2 of the plan). Stronger than
///   `Entailed`, still not an unconditional guarantee (two-statement
///   equivalence is undecidable in general).
/// - [`TrustRung::Discharged`] — a Lean proof of the decl exists (the
///   machine English is generated from a kernel-checked term).
///
/// Honest labelling is a hard requirement (SPEC §4): never assign a rung
/// that overstates what was actually checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustRung {
    Bare,
    Targeted,
    Entailed,
    EntailedFormal,
    Discharged,
}

impl fmt::Display for TrustRung {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TrustRung::Bare => "warrant:bare",
            TrustRung::Targeted => "warrant:targeted",
            TrustRung::Entailed => "warrant:entailed",
            TrustRung::EntailedFormal => "warrant:entailed-formal",
            TrustRung::Discharged => "discharged",
        };
        f.write_str(s)
    }
}

/// The outcome of an entailment check: does the target passage support the
/// machine-English claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Judgement {
    /// The passage entails the claim.
    Entailed,
    /// The passage contradicts, or does not support, the claim.
    NotEntailed,
    /// The check could not confidently decide either way (e.g. the stub's
    /// lexical-overlap heuristic fell below threshold, or the judge itself
    /// reported low confidence).
    Uncertain,
}

impl fmt::Display for Judgement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Judgement::Entailed => "entailed",
            Judgement::NotEntailed => "not_entailed",
            Judgement::Uncertain => "uncertain",
        };
        f.write_str(s)
    }
}

/// A reviewable diagnostic for one warrant: the resolved target passage,
/// the machine-English rendering of the checked declaration, the
/// entailment judgement obtained between them, the resulting trust rung,
/// and a rationale a human reviewer can audit.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// Fully-qualified Lean declaration name.
    pub decl: String,
    /// Which literature source this warrant cites (e.g. `"evans-2010"`).
    pub source_id: String,
    /// The citation locator as given in the manifest (e.g. `"§6.3.1"`).
    pub locator: String,
    /// The resolved passage text the claim was checked against.
    pub target_passage: String,
    /// The deterministic, faithful-by-construction English rendering of
    /// the Lean declaration's type (from `LeanDeclInfo::type_english`).
    pub machine_english: String,
    /// Whether the passage was found to entail the machine English.
    pub judgement: Judgement,
    /// This warrant's rung in the obligation lattice.
    pub trust_rung: TrustRung,
    /// A one-line-or-so human-readable explanation of the judgement.
    /// For [`crate::entail::StubEntailment`] this always says "stub" —
    /// it is a test double, never a real check.
    pub rationale: String,
    /// The entailment judge's confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

/// Render a [`Verdict`] as human-readable CLI text.
pub fn render_report(v: &Verdict) -> String {
    format!(
        "warrant: {decl}\n\
         source: {source_id} ({locator})\n\
         trust rung: {trust_rung}\n\
         judgement: {judgement} (confidence {confidence:.2})\n\
         rationale: {rationale}\n\
         \n\
         machine english:\n  {machine_english}\n\
         \n\
         target passage:\n  {target_passage}\n",
        decl = v.decl,
        source_id = v.source_id,
        locator = v.locator,
        trust_rung = v.trust_rung,
        judgement = v.judgement,
        confidence = v.confidence,
        rationale = v.rationale,
        machine_english = v.machine_english,
        target_passage = v.target_passage,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_rung_display_matches_spec_lattice_strings() {
        assert_eq!(TrustRung::Bare.to_string(), "warrant:bare");
        assert_eq!(TrustRung::Targeted.to_string(), "warrant:targeted");
        assert_eq!(TrustRung::Entailed.to_string(), "warrant:entailed");
        assert_eq!(
            TrustRung::EntailedFormal.to_string(),
            "warrant:entailed-formal"
        );
        assert_eq!(TrustRung::Discharged.to_string(), "discharged");
    }

    #[test]
    fn judgement_display_matches_spec_data_contract() {
        assert_eq!(Judgement::Entailed.to_string(), "entailed");
        assert_eq!(Judgement::NotEntailed.to_string(), "not_entailed");
        assert_eq!(Judgement::Uncertain.to_string(), "uncertain");
    }

    #[test]
    fn render_report_includes_key_fields() {
        let v = Verdict {
            decl: "Foo.bar".to_string(),
            source_id: "evans-2010".to_string(),
            locator: "§6.3.1".to_string(),
            target_passage: "the passage text".to_string(),
            machine_english: "the machine english".to_string(),
            judgement: Judgement::Entailed,
            trust_rung: TrustRung::Entailed,
            rationale: "stub: matched".to_string(),
            confidence: 0.75,
        };
        let report = render_report(&v);
        assert!(report.contains("Foo.bar"));
        assert!(report.contains("evans-2010"));
        assert!(report.contains("warrant:entailed"));
        assert!(report.contains("entailed"));
        assert!(report.contains("the passage text"));
        assert!(report.contains("the machine english"));
    }
}
