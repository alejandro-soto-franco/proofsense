/-
proofsense-lean: deterministic statement verbaliser.

`verbalizeType : Expr → MetaM String` folds a declaration's type `Expr` into
faithful English via a small trusted reading table, one canonical reading per
head symbol. Every rule is meaning-preserving. Any subterm no rule matches is
rendered verbatim via `ppExpr` (still faithful, just less English), so the fold
never crashes -- in particular proof-irrelevance elisions (`⋯`) live inside such
verbatim fragments and pretty-print exactly as they do in `type_pp`.

Pure metaprogramming; deterministic; no network, no LLM. This is the
faithful-by-construction generator.

This module imports only Lean core, not Mathlib, so Mathlib-specific head
symbols (`Filter.Eventually`, measure-`ae`, `IsCompact`, …) are referenced by
UNCHECKED single-backtick `Name` literals and matched with equality guards.
Core logical/order constants use checked ``-literals.
-/
import Lean

open Lean Meta

namespace Proofsense.Verbalize

/-- Pretty-print a subterm verbatim (the universal faithful fallback). This is
    where `⋯` proof-irrelevance placeholders render, exactly as in `type_pp`. -/
def pp (e : Expr) : MetaM String := do
  return toString (← Lean.PrettyPrinter.ppExpr e)

mutual

/-- Deterministic fold: type `Expr` → English. One canonical reading per head
    symbol; unmatched subterms fall back to `pp` (verbatim `ppExpr`). -/
partial def verbalizeType (e : Expr) : MetaM String := do
  match e with
  -- Universal / implication: `∀`. A `Prop` domain reads as an implication
  -- ("if … then …") since the hypothesis is proof-irrelevant; a data domain
  -- reads as a genuine universal ("for all x : T, …").
  | .forallE n dom body bi =>
    if ← isProp dom then
      withLocalDecl n bi dom fun x => do
        let d ← verbalizeType dom
        let b ← verbalizeType (body.instantiate1 x)
        return s!"if {d}, then {b}"
    else
      withLocalDecl n bi dom fun x => do
        let dS ← pp dom
        let b ← verbalizeType (body.instantiate1 x)
        return s!"for all {n.eraseMacroScopes} : {dS}, {b}"
  | _ =>
    let (head, args) := e.getAppFnArgs
    match head, args with
    -- Existential: `∃ x : T, P x`.
    | ``Exists, #[_, .lam n t b bi] =>
      withLocalDecl n bi t fun x => do
        let tS ← pp t
        let bS ← verbalizeType (b.instantiate1 x)
        return s!"there exists {n.eraseMacroScopes} : {tS} such that {bS}"
    -- Propositional connectives (all Lean core).
    | ``And, #[a, b] => return s!"{← verbalizeType a} and {← verbalizeType b}"
    | ``Or,  #[a, b] => return s!"{← verbalizeType a} or {← verbalizeType b}"
    | ``Iff, #[a, b] => return s!"{← verbalizeType a} if and only if {← verbalizeType b}"
    | ``Not, #[a]    => return s!"it is not the case that {← verbalizeType a}"
    -- (In)equalities and order (all Lean core).
    | ``Eq,    #[_, a, b]    => return s!"{← verbalizeType a} equals {← verbalizeType b}"
    | ``LE.le, #[_, _, a, b] => return s!"{← verbalizeType a} is at most {← verbalizeType b}"
    | ``LT.lt, #[_, _, a, b] => return s!"{← verbalizeType a} is less than {← verbalizeType b}"
    | ``GE.ge, #[_, _, a, b] => return s!"{← verbalizeType a} is at least {← verbalizeType b}"
    | ``GT.gt, #[_, _, a, b] => return s!"{← verbalizeType a} is greater than {← verbalizeType b}"
    -- Everything else: Mathlib-specific heads (unchecked names) then the
    -- universal faithful `ppExpr` fallback. Notation unexpanders fire in `pp`,
    -- so `‖·‖`, `∫`, `∈`, `⊆`, `*`, `+`, `↑`, and any `⋯` render exactly as in
    -- `type_pp`.
    | _, _ => verbalizeMathlib e head args

/-- Mathlib-specific readings, keyed by unchecked `Name` equality so this file
    need not import Mathlib. Falls back to verbatim `pp e`. -/
partial def verbalizeMathlib (e : Expr) (head : Name) (args : Array Expr) : MetaM String := do
  -- Almost-everywhere: `∀ᵐ x ∂μ, P x` = `Filter.Eventually P (ae μ)`.
  if head == `Filter.Eventually then
    match args with
    | #[_, .lam n t b bi, filt] =>
      if filt.getAppFnArgs.1 == `MeasureTheory.Measure.ae then
        withLocalDecl n bi t fun x => do
          let tS ← pp t
          let bS ← verbalizeType (b.instantiate1 x)
          return s!"for almost every {n.eraseMacroScopes} : {tS}, {bS}"
      else pp e
    | _ => pp e
  -- Set inclusion: `A ⊆ B`.
  else if head == `HasSubset.Subset then
    match args with
    | #[_, _, a, b] => return s!"{← verbalizeType a} is a subset of {← verbalizeType b}"
    | _ => pp e
  -- Small trusted predicate phrase table (subject is the last argument).
  else if head == `IsCompact then return s!"{← pp args.back!} is compact"
  else if head == `IsOpen then return s!"{← pp args.back!} is open"
  else if head == `IsClosed then return s!"{← pp args.back!} is closed"
  else if head == `MeasurableSet then return s!"{← pp args.back!} is measurable"
  -- Named-constant applications, arithmetic, norms, integrals, membership,
  -- projections, fvars, literals, …: faithful verbatim `ppExpr`.
  else pp e

end

end Proofsense.Verbalize
