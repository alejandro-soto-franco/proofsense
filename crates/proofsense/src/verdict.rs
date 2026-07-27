//! Verdict types: the trust-rung lattice, the directional entailment
//! answers, the relation derived from them, the per-warrant [`Verdict`]
//! record, and human-readable report rendering.
//!
//! See the SPEC's "Trust model: the obligation lattice" (§4): every warrant
//! is labelled with exactly how strong its check is. `TrustRung::Entailed`
//! is evidence-grade (default LLM-NLI against a faithful, deterministically
//! generated operand). Never conflate it with a soundness proof.
//! `TrustRung::EntailedFormal` is reserved for the (future) round-trip
//! re-autoformalisation + in-kernel discharge; it is strictly stronger and
//! must never be assigned by the evidence-grade path.

use crate::manifest::Claim;
use serde::Serialize;
use std::fmt;

/// A warrant's position in the obligation lattice, from weakest to
/// strongest. Mirrors SPEC §4 exactly:
///
/// - [`TrustRung::Bare`]: a citation only; no target passage resolved.
/// - [`TrustRung::Targeted`]: the locator resolved to a specific
///   transcribed passage, and the relation derived from the two directional
///   answers falls short of the warrant's claim. A [`Defect`] accompanies it
///   in every case except [`Relation::Undetermined`], so this rung reads as
///   "checked, and here is what the check found". Every finding of
///   misattribution lands here.
/// - [`TrustRung::Entailed`]: the derived relation supports what the warrant
///   claims: [`Relation::Equivalent`] under either claim, or
///   [`Relation::DeclSpecialises`] under [`Claim::FollowsFrom`].
///   **Evidence-grade**: the default is LLM-NLI with the faithful operand
///   pinned. This is evidence, not proof.
/// - [`TrustRung::EntailedFormal`]: the passage was re-autoformalised and
///   discharged equivalent to the decl in-kernel (**formal-evidence-grade**:
///   a round-trip check, Increment 2 of the plan). Stronger than
///   `Entailed`, still not an unconditional guarantee (two-statement
///   equivalence is undecidable in general).
/// - [`TrustRung::Discharged`]: a Lean proof of the decl exists (the
///   machine English is generated from a kernel-checked term).
///
/// Accurate labelling is a hard requirement (SPEC §4): never assign a rung
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

/// Serialises as the exact SPEC §4 lattice string (e.g. `"warrant:entailed"`),
/// matching [`fmt::Display`] rather than the Rust variant name.
impl Serialize for TrustRung {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// One directional entailment answer: does the premise the judge was given
/// entail the hypothesis it was given. The judge is asked this twice with the
/// operands swapped; it is never asked to classify the relation itself.
#[derive(Debug, Clone, Serialize)]
pub struct Directional {
    /// Whether the premise entails the hypothesis.
    pub holds: bool,
    /// The judge's confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// A one-line explanation a human reviewer can audit.
    pub rationale: String,
}

/// How a declaration stands to the passage it cites. Derived from the two
/// directional answers, never asked for directly.
///
/// Every reading below assumes the passage is the cited *statement*. Where a
/// locator resolves to a whole section carrying several theorems, remarks and
/// definitions, the second direction asks one declaration to entail all of
/// it, which is answered `false` almost always. That collapses
/// [`Relation::Equivalent`] into [`Relation::DeclSpecialises`] and puts
/// [`Relation::DeclExceeds`] out of reach. Measured over thirteen blind
/// pairings against section-granularity passages, `Equivalent` came back
/// zero times. Against a section, read `DeclSpecialises` as "the section says
/// more", a fact about the operand rather than about the declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    /// Each entails the other: the declaration states the cited result.
    Equivalent,
    /// The source entails the declaration only: the declaration is a special
    /// case of the cited result, so citing it unqualified understates the
    /// source and overstates the declaration's generality.
    DeclSpecialises,
    /// The declaration entails the source only: the declaration claims more
    /// than the source supports. Unsound under either claim.
    DeclExceeds,
    /// Neither direction holds: the passage does not establish the claim.
    Divergent,
    /// At least one direction came back below the judge's confidence floor.
    Undetermined,
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Relation::Equivalent => "equivalent",
            Relation::DeclSpecialises => "decl_specialises",
            Relation::DeclExceeds => "decl_exceeds",
            Relation::Divergent => "divergent",
            Relation::Undetermined => "undetermined",
        };
        f.write_str(s)
    }
}

/// Serialises as the data-contract string (e.g. `"decl_specialises"`),
/// matching [`fmt::Display`] rather than the Rust variant name.
impl Serialize for Relation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl Relation {
    /// Derive the relation from the two directional answers. `floor` is the
    /// judge's confidence floor: if either direction is answered below it, the
    /// relation is [`Relation::Undetermined`] regardless of the answers.
    pub fn derive(
        source_entails_decl: &Directional,
        decl_entails_source: &Directional,
        floor: f32,
    ) -> Relation {
        if source_entails_decl.confidence < floor || decl_entails_source.confidence < floor {
            return Relation::Undetermined;
        }
        match (source_entails_decl.holds, decl_entails_source.holds) {
            (true, true) => Relation::Equivalent,
            (true, false) => Relation::DeclSpecialises,
            (false, true) => Relation::DeclExceeds,
            (false, false) => Relation::Divergent,
        }
    }
}

/// A positive finding about a warrant, kept separate from the rung so that
/// [`TrustRung`] stays a monotone strength label and a finding of
/// misattribution never has to present itself as weak evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Defect {
    /// The declaration is weaker than the result it cites. Subject to the
    /// operand-granularity caveat on [`Relation`]: against a section-wide
    /// passage this fires on faithful formalisations too.
    Understated,
    /// The declaration is stronger than the source supports.
    Overclaimed,
    /// Neither direction holds, so the citation establishes nothing.
    Unsupported,
}

impl fmt::Display for Defect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Defect::Understated => "understated",
            Defect::Overclaimed => "overclaimed",
            Defect::Unsupported => "unsupported",
        };
        f.write_str(s)
    }
}

/// Serialises as the data-contract string (e.g. `"understated"`), matching
/// [`fmt::Display`] rather than the Rust variant name.
impl Serialize for Defect {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Map a warrant's claim and its derived relation onto a trust rung and, when
/// there is one, a defect.
///
/// A locator that fails to resolve never reaches this function: it yields
/// [`TrustRung::Bare`] at the call site, because there is no passage against
/// which any relation could be derived.
pub fn classify(claim: Claim, relation: Relation) -> (TrustRung, Option<Defect>) {
    match (claim, relation) {
        (_, Relation::Equivalent) => (TrustRung::Entailed, None),
        (Claim::FollowsFrom, Relation::DeclSpecialises) => (TrustRung::Entailed, None),
        (Claim::Formalises, Relation::DeclSpecialises) => {
            (TrustRung::Targeted, Some(Defect::Understated))
        }
        (_, Relation::DeclExceeds) => (TrustRung::Targeted, Some(Defect::Overclaimed)),
        (_, Relation::Divergent) => (TrustRung::Targeted, Some(Defect::Unsupported)),
        (_, Relation::Undetermined) => (TrustRung::Targeted, None),
    }
}

/// A reviewable diagnostic for one warrant.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    /// Fully-qualified Lean declaration name.
    pub decl: String,
    /// Which literature source this warrant cites (e.g. `"evans-2010"`).
    pub source_id: String,
    /// The citation locator as given in the manifest (e.g. `"§6.3.1"`).
    pub locator: String,
    /// What the warrant claims about the correspondence.
    pub claim: Claim,
    /// How the declaration stands to the cited passage.
    pub relation: Relation,
    /// This warrant's rung in the obligation lattice.
    pub trust_rung: TrustRung,
    /// A positive finding, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defect: Option<Defect>,
    /// Does the passage entail the machine English? Absent when the locator
    /// did not resolve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_entails_decl: Option<Directional>,
    /// Does the machine English entail the passage? Absent when the locator
    /// did not resolve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decl_entails_source: Option<Directional>,
    /// The checked declaration's axiom dependencies.
    pub decl_axioms: Vec<String>,
    /// Whether the declaration's proof is free of `sorry`.
    pub sorry_free: bool,
    /// SHA-256 of the resolved passage text. Absent when nothing resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage_sha256: Option<String>,
    /// The deterministic, faithful-by-construction English rendering of the
    /// Lean declaration's type.
    pub machine_english: String,
    /// The resolved passage text the claim was checked against.
    pub target_passage: String,
}

/// Render a [`Verdict`] as human-readable CLI text.
pub fn render_report(v: &Verdict) -> String {
    let defect = match v.defect {
        Some(d) => format!("\ndefect: {d}"),
        None => String::new(),
    };
    let directions = match (&v.source_entails_decl, &v.decl_entails_source) {
        (Some(s), Some(d)) => format!(
            "\nsource entails decl: {} (confidence {:.2}) {}\
             \ndecl entails source: {} (confidence {:.2}) {}",
            s.holds, s.confidence, s.rationale, d.holds, d.confidence, d.rationale
        ),
        _ => String::new(),
    };

    format!(
        "warrant: {decl}\n\
         source: {source_id} ({locator})\n\
         claim: {claim}\n\
         relation: {relation}\n\
         trust rung: {trust_rung}{defect}{directions}\n\
         \n\
         machine english:\n  {machine_english}\n\
         \n\
         target passage:\n  {target_passage}\n",
        decl = v.decl,
        source_id = v.source_id,
        locator = v.locator,
        claim = v.claim,
        relation = v.relation,
        trust_rung = v.trust_rung,
        defect = defect,
        directions = directions,
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

    fn sample_verdict() -> Verdict {
        Verdict {
            decl: "Foo.bar".to_string(),
            source_id: "evans-2010".to_string(),
            locator: "§6.3.1".to_string(),
            claim: Claim::Formalises,
            relation: Relation::DeclSpecialises,
            trust_rung: TrustRung::Targeted,
            defect: Some(Defect::Understated),
            source_entails_decl: Some(dir(true, 0.94)),
            decl_entails_source: Some(dir(false, 0.88)),
            decl_axioms: vec!["propext".to_string()],
            sorry_free: true,
            passage_sha256: Some("a1c9".to_string()),
            machine_english: "the machine english".to_string(),
            target_passage: "the passage text".to_string(),
        }
    }

    #[test]
    fn render_report_shows_the_relation_and_the_defect() {
        let report = render_report(&sample_verdict());
        assert!(report.contains("Foo.bar"));
        assert!(report.contains("warrant:targeted"));
        assert!(report.contains("decl_specialises"));
        assert!(report.contains("understated"));
        assert!(report.contains("formalises"));
        assert!(report.contains("the passage text"));
        assert!(report.contains("the machine english"));
    }

    #[test]
    fn a_verdict_serialises_with_the_data_contract_strings() {
        let json = serde_json::to_value(sample_verdict()).unwrap();
        assert_eq!(json["claim"], "formalises");
        assert_eq!(json["relation"], "decl_specialises");
        assert_eq!(json["trust_rung"], "warrant:targeted");
        assert_eq!(json["defect"], "understated");
        assert_eq!(json["source_entails_decl"]["holds"], true);
        assert_eq!(json["decl_entails_source"]["holds"], false);
    }

    /// An unresolved locator has no passage, so there is nothing to hash and
    /// no direction to answer. Those fields are absent rather than empty.
    #[test]
    fn an_unresolved_warrant_omits_the_passage_fields() {
        let mut v = sample_verdict();
        v.trust_rung = TrustRung::Bare;
        v.relation = Relation::Undetermined;
        v.defect = None;
        v.source_entails_decl = None;
        v.decl_entails_source = None;
        v.passage_sha256 = None;
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("source_entails_decl").is_none());
        assert!(json.get("decl_entails_source").is_none());
        assert!(json.get("passage_sha256").is_none());
        assert!(json.get("defect").is_none());
    }

    fn dir(holds: bool, confidence: f32) -> Directional {
        Directional {
            holds,
            confidence,
            rationale: "t".to_string(),
        }
    }

    #[test]
    fn relation_derives_from_the_pair_of_directions() {
        assert_eq!(
            Relation::derive(&dir(true, 0.9), &dir(true, 0.9), 0.5),
            Relation::Equivalent
        );
        assert_eq!(
            Relation::derive(&dir(true, 0.9), &dir(false, 0.9), 0.5),
            Relation::DeclSpecialises
        );
        assert_eq!(
            Relation::derive(&dir(false, 0.9), &dir(true, 0.9), 0.5),
            Relation::DeclExceeds
        );
        assert_eq!(
            Relation::derive(&dir(false, 0.9), &dir(false, 0.9), 0.5),
            Relation::Divergent
        );
    }

    #[test]
    fn either_direction_below_the_floor_is_undetermined() {
        assert_eq!(
            Relation::derive(&dir(true, 0.4), &dir(true, 0.9), 0.5),
            Relation::Undetermined
        );
        assert_eq!(
            Relation::derive(&dir(true, 0.9), &dir(false, 0.4), 0.5),
            Relation::Undetermined
        );
    }

    #[test]
    fn a_zero_floor_never_yields_undetermined() {
        assert_eq!(
            Relation::derive(&dir(true, 0.0), &dir(true, 0.0), 0.0),
            Relation::Equivalent
        );
    }

    #[test]
    fn specialisation_is_a_defect_only_under_the_formalises_claim() {
        assert_eq!(
            classify(Claim::Formalises, Relation::DeclSpecialises),
            (TrustRung::Targeted, Some(Defect::Understated))
        );
        assert_eq!(
            classify(Claim::FollowsFrom, Relation::DeclSpecialises),
            (TrustRung::Entailed, None)
        );
    }

    #[test]
    fn exceeding_the_source_is_a_defect_under_both_claims() {
        assert_eq!(
            classify(Claim::Formalises, Relation::DeclExceeds),
            (TrustRung::Targeted, Some(Defect::Overclaimed))
        );
        assert_eq!(
            classify(Claim::FollowsFrom, Relation::DeclExceeds),
            (TrustRung::Targeted, Some(Defect::Overclaimed))
        );
    }

    #[test]
    fn equivalence_reaches_entailed_and_divergence_does_not() {
        assert_eq!(
            classify(Claim::Formalises, Relation::Equivalent),
            (TrustRung::Entailed, None)
        );
        assert_eq!(
            classify(Claim::Formalises, Relation::Divergent),
            (TrustRung::Targeted, Some(Defect::Unsupported))
        );
    }

    #[test]
    fn undetermined_is_targeted_without_a_defect() {
        assert_eq!(
            classify(Claim::Formalises, Relation::Undetermined),
            (TrustRung::Targeted, None)
        );
    }

    #[test]
    fn relation_and_defect_display_as_the_data_contract_strings() {
        assert_eq!(Relation::Equivalent.to_string(), "equivalent");
        assert_eq!(Relation::DeclSpecialises.to_string(), "decl_specialises");
        assert_eq!(Relation::DeclExceeds.to_string(), "decl_exceeds");
        assert_eq!(Relation::Divergent.to_string(), "divergent");
        assert_eq!(Relation::Undetermined.to_string(), "undetermined");
        assert_eq!(Defect::Understated.to_string(), "understated");
        assert_eq!(Defect::Overclaimed.to_string(), "overclaimed");
        assert_eq!(Defect::Unsupported.to_string(), "unsupported");
    }
}
