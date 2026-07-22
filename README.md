# proofsense

proofsense is a proof-linter: it checks that a Lean 4 proof corresponds to the
literature it claims to formalise.

A Lean proof can typecheck and still misrepresent its source: a hypothesis
silently strengthened, a step that cites a lemma the source never states, an
inequality with the wrong direction. Lean answers "does this typecheck".
proofsense targets the other question, "does this match what the ingested paper
actually says".

## Why the generator is separated from the judge

The obvious way to build this is to ask a model whether a Lean declaration
matches a paper. That fails for a reason no amount of prompting fixes: the model
produces both the reading of the Lean and the verdict, so a misreading paired
with a confirming verdict is indistinguishable from a correct pair.

proofsense splits the two.

The English rendering of a declaration is produced by `verbalizeType : Expr →
MetaM String`, a deterministic fold over the declaration's own type expression
inside `MetaM`. One canonical reading per head symbol, drawn from a small trusted
reading table. Any subterm no rule matches is rendered verbatim through
`ppExpr`, so the fold is total: it degrades to less English, never to a different
claim. It is pure metaprogramming, with no network and no model involved.

The judge therefore never writes the claim. It only decides whether the source
supports a claim derived mechanically from the term under check. A wrong
judgement stays a wrong judgement; it cannot become a wrong reading of the Lean.

## Pipeline

1. **Ingest.** Reviewable literature is OCR'd (MinerU) into a reading table: a
   structured, citable record of the source's definitions, hypotheses, and
   theorem statements.
2. **Resolve.** A warrant's locator (`§6.3.1`) is matched to a passage by string
   equality after normalisation. No embeddings, no fuzzy matching, so resolution
   is reproducible and inspectable.
3. **Verbalise.** The Lean declaration under check is rendered into English by
   the deterministic fold described above.
4. **Entail.** The rendered claim is checked against the resolved passage.
5. **Label.** The warrant is assigned a rung on the trust ladder.

## The trust ladder

Every warrant is labelled with exactly how strong its check was. Never assigning
a rung that overstates what was actually checked is a hard requirement, not a
preference.

| Rung | Meaning |
|---|---|
| `Bare` | A citation only. The locator resolved to no passage. |
| `Targeted` | The locator resolved to a specific passage, entailment unconfirmed. |
| `Entailed` | The passage was entailment-checked against the rendered claim and supported it. |
| `EntailedFormal` | The passage was re-autoformalised and checked for equivalence. Still not unconditional: two-statement equivalence is undecidable in general. |
| `Discharged` | A Lean proof of the declaration exists, so the English is generated from a kernel-checked term. |

## What is guaranteed, and what is not

Verbalisation is faithful by construction: the English is a direct function of
the Lean declaration, so it cannot introduce claims the proof does not make.

The entailment check is evidence-grade, not a soundness proof. It reports whether
a judge finds the source consistent with the rendered claim, and that judgement
can be wrong in either direction. proofsense flags mismatches for human review.
It does not certify correctness.

## Trusted computing base

Results are only as good as three components proofsense does not itself verify:

- **The reading table**: its coverage, and its transcription of the source.
- **MinerU OCR fidelity**: the accuracy of the ingest step that produces the
  reading table from source documents.
- **The entailment judge**: the model or procedure deciding whether the reading
  table supports the rendered claim.

A false negative or false positive in any of the three propagates into
proofsense's output.

## Usage

A manifest names the sources, the Lean imports to elaborate under, and the
warrants (a declaration plus the locator it claims to formalise).

```
# Deterministic and offline: uses the lexical stub judge.
cargo run -- check path/to/manifest.json --stub

# With the LLM judge. Requires both variables; absent either, no request is made.
PROOFSENSE_ENABLE_LLM=1 ANTHROPIC_API_KEY=... \
  cargo run -- check path/to/manifest.json

# Skip spawning Lean by supplying a captured declaration record.
cargo run -- check path/to/manifest.json --stub --lean-info decl.json
```

Entailment backends:

- `StubEntailment`: a deterministic lexical-overlap heuristic. A test double, not
  a real check. Its rationale always begins with `stub:`, so a stub verdict can
  never be mistaken for a judged one.
- `LlmEntailment`: an NLI call to an LLM judge, speaking the Anthropic Messages
  API directly. Gated behind `PROOFSENSE_ENABLE_LLM`, which is what keeps the
  test suite fully offline: no test sets that variable.

## Worked example

The fixtures check `EllipticDirichlet.Regularity.interior_H2_estimate`, the
interior H2 estimate from a Lean formalisation of the linear elliptic Dirichlet
problem, against Evans, *Partial Differential Equations*, section 6.3.1, the
textbook result it claims to formalise.

## Status

Implemented and covered by tests: ingest, locator resolution, the Lean bridge,
deterministic verbalisation, both entailment backends, verdict labelling, and the
end-to-end orchestrator. 15 tests, all offline.

Not yet implemented: the `EntailedFormal` and `Discharged` rungs are defined in
the lattice but no code path produces them. Checking is statement-level, one
warrant at a time, and does not descend into proof steps. The verbaliser's
reading table covers core logical and order constants through checked name
literals, plus a set of Mathlib head symbols matched by unchecked name literals,
with everything else falling back to verbatim pretty-printing.

## Licence

Apache-2.0.
