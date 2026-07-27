# proofsense

proofsense checks that a Lean 4 declaration states the theorem it cites, and
reports how the two stand to each other.

Formal verification establishes that a proof proves its statement. It
establishes nothing about whether that statement is the theorem the author says
it is, and that second link is where a formalisation goes wrong: a hypothesis
silently strengthened, a step citing a lemma the source never states, an
inequality with the wrong direction. Lean answers "does this typecheck".
proofsense answers "is this the result the cited literature actually proves".

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

The fold keeps every binder to which the statement refers. A `Prop` hypothesis
reads as an implication only when the body does not mention it; when the body
projects out of it, the hypothesis is named instead, so no identifier appears in
the English without having been introduced.

The judge therefore never writes the claim. It only decides whether the source
supports a claim derived mechanically from the term under check. A wrong
judgement stays a wrong judgement; it cannot become a wrong reading of the Lean.

The judge is never asked which relation holds either. It answers one direction
at a time: does the passage entail the declaration, and, as a separate call,
does the declaration entail the passage. proofsense derives the relation from
the two answers. Asking for the relation directly lets a single call report
"matches" for a declaration that quietly assumes more than the source gives,
which is the case a binary verdict has no way to express.

## Pipeline

1. **Ingest.** Reviewable literature is OCR'd (MinerU) into a reading table: a
   structured, citable record of the source's definitions, hypotheses, and
   theorem statements. Both of MinerU's outputs are read, `content_list.json`
   and the Markdown export, dispatched by file extension. The Markdown export
   carries no heading markers, so its structure is recovered from line form: a
   section heading needs a dotted number of at least two components, which keeps
   a numbered list item from opening a section.
2. **Resolve.** A warrant's locator is matched to a passage by string equality
   after normalisation. No embeddings, no fuzzy matching, so resolution is
   reproducible and inspectable. A locator names either a section (`§6.3.1`) or
   one statement inside it (`§6.3.1 Thm 1`).
3. **Verbalise.** The Lean declaration under check is rendered into English by
   the deterministic fold described above.
4. **Relate.** The rendered claim and the resolved passage are checked in both
   directions, and the relation between them is derived from the two answers.
5. **Label.** The relation is mapped, against what the warrant claims, to a rung
   on the trust ladder and an optional defect.

## What a warrant claims, and the relation

A warrant names a declaration, a source and a locator, and declares what it
claims about the correspondence:

| `claim` | Reading |
|---|---|
| `formalises` | this declaration *is* the cited theorem |
| `follows_from` | this step is justified by the source |

The field defaults to `formalises`, the stricter reading, so an unstated claim is
never the lenient one.

The two directional answers give four relations, and a fifth for an answer below
the judge's confidence floor:

| Relation | Meaning |
|---|---|
| `equivalent` | each entails the other: the declaration states the cited result |
| `decl_specialises` | the source entails the declaration only: a special case of the cited result |
| `decl_exceeds` | the declaration entails the source only: it claims more than the source supports |
| `divergent` | neither direction holds |
| `undetermined` | a direction came back below the confidence floor |

The same relation classifies differently under each claim, which is the reason
the claim is recorded at all:

| Relation | `formalises` | `follows_from` |
|---|---|---|
| `equivalent` | `Entailed` | `Entailed` |
| `decl_specialises` | `Targeted` + `Understated` | `Entailed` |
| `decl_exceeds` | `Targeted` + `Overclaimed` | `Targeted` + `Overclaimed` |
| `divergent` | `Targeted` + `Unsupported` | `Targeted` + `Unsupported` |
| `undetermined` | `Targeted` | `Targeted` |
| locator unresolved | `Bare` | `Bare` |

A defect is a positive finding, recorded separately from the rung so that the
rung stays a monotone strength label and a finding of misattribution never has to
present itself as weak evidence.

## The trust ladder

Every warrant is labelled with exactly how strong its check was. Never assigning
a rung that overstates what was actually checked is a hard requirement, not a
preference.

| Rung | Meaning |
|---|---|
| `Bare` | A citation only. The locator resolved to no passage. |
| `Targeted` | The locator resolved and a relation was derived, and the relation falls short of what the warrant claims. A defect accompanies it in every case except `undetermined`. |
| `Entailed` | The derived relation supports the claim: `equivalent` under either claim, or `decl_specialises` under `follows_from`. |
| `EntailedFormal` | The passage was re-autoformalised and checked for equivalence. Still not unconditional: two-statement equivalence is undecidable in general. |
| `Discharged` | A Lean proof of the declaration exists, so the English is generated from a kernel-checked term. |

### What the ladder currently reaches

A locator that names a section resolves to a passage carrying several theorems,
remarks and definitions, and the second direction then asks one declaration to
entail all of it. That is answered `false` almost always, so the relation
collapses to `decl_specialises` whenever the first direction holds. Across the
thirteen pairings described under `LlmEntailment` below, all judged against
section-granularity passages, `equivalent` came back zero times.

A locator may now name the statement it means, `§6.3.1 Thm 1` rather than
`§6.3.1`, which removes the operand-scope reason those two relations were out of
reach. Measured over the nine warrants of a development checked against Evans,
section granularity hands the judge 88,249 characters where statement
granularity hands it 7,617, a factor of 11.6. Section 6.3.1 alone runs to 12,555
characters over three theorems, and the theorem a declaration there formalises
is 1,304.

What that buys has not been re-measured. The thirteen-pairing run above predates
statement granularity, and whether the judge returns `equivalent` once handed the
statement is an open measurement rather than a result. Until it is run, treat the
zero above as a fact about section-granularity operands.

`EntailedFormal` and `Discharged` remain defined and unreachable. No code path
produces either.

## What is and isn't guaranteed

Verbalisation is faithful by construction: the English is a direct function of
the Lean declaration, so it cannot introduce claims the proof does not make.

The entailment check is evidence-grade, not a soundness proof. It reports what a
judge made of each direction, and either answer can be wrong. Deriving the
relation from two answers removes one failure mode, a single call reporting
"matches" for a declaration that assumes more than the source gives. The answers
themselves stay as reliable as the judge. proofsense flags mismatches for human
review. It does not certify correctness.

## The report

A run returns a `proofsense.report/1` document rather than a list of verdicts.
It pins what was read and what judged it:

- the SHA-256 of the manifest bytes;
- the Lean toolchain, the subject package and its revision, and every package
  revision the Lake manifest pins;
- per source, the SHA-256 of the content-list bytes and the passage count;
- per verdict, the SHA-256 of the resolved passage text, the declaration's axiom
  dependencies, and whether its proof is free of `sorry`;
- the judge: kind, model, effort, confidence floor, and the SHA-256 of the prompt
  template, since a prompt change alters verdicts and has to be visible.

The judge block is filled in by whichever backend ran, so a report cannot
describe a judge that never ran. A stub run carries no model, effort or prompt
hash at all.

Signing is out of scope. A detached signature over the document needs no schema
change.

## Trusted computing base

Results are only as good as three components proofsense does not itself verify:

- **The reading table**: its coverage, and its transcription of the source.
- **MinerU OCR fidelity**: the accuracy of the ingest step that produces the
  reading table from source documents.
- **The entailment judge**: the model or procedure deciding each direction.

A false negative or false positive in any of the three propagates into
proofsense's output.

## Requirements

`proofsense check` elaborates the Lean declaration by spawning `lake exe
proofsense-lean` from the Lake project under `lean/`, so a Lean 4 toolchain and
that project must both be present. A downloaded release binary carries neither.

The Lake project requires the subject at a pinned revision, and **that pin
decides what this tool reports**. Pinned one revision too early, the subject
carried an `interior_H2_estimate` with drift-free hypotheses its author had
already removed, so a run would have verbalised a superseded statement and
re-reported findings the development had closed. Bump the pin whenever the
subject's statements move.

`--lean-info decl.json` is the one route that needs no Lean toolchain. It reads
a declaration record captured earlier by whoever did have Lean available, as
emitted by `lake exe proofsense-lean`.

`--stub` is a separate axis: it selects the deterministic lexical judge over the
LLM one, so it removes the network and the API key, and has no bearing on
whether Lean is spawned.

## Usage

A manifest names the sources, the Lean imports to elaborate under, and the
warrants (a declaration, the locator it cites, and what it claims about it).

```
# Deterministic and offline: uses the lexical stub judge.
cargo run -- check path/to/manifest.json --stub

# With the LLM judge. Requires both variables; absent either, no request is made.
PROOFSENSE_ENABLE_LLM=1 ANTHROPIC_API_KEY=... \
  cargo run -- check path/to/manifest.json

# Skip spawning Lean by supplying a captured declaration record.
cargo run -- check path/to/manifest.json --stub --lean-info decl.json

# Check only that every locator resolves. No Lean, no judge, no network.
cargo run -- resolve path/to/manifest.json
```

`resolve` reports the passage each locator matched, its size in characters and
its opening line, and exits non-zero on a miss. A warrant pointing at the wrong
passage yields a verdict about the wrong theorem, and no amount of judging
recovers from that, so this gates a manifest before any expensive step runs.

Entailment backends:

- `StubEntailment`: a deterministic lexical-overlap heuristic, and a test double
  rather than a real check. It answers both directions identically by
  construction, so it returns only `equivalent` or `divergent` and the one-sided
  relations are invisible to it. Its rationale always begins with `stub:`, so a
  stub verdict can never be mistaken for a judged one.

  It scores the fraction of the passage's salient tokens the machine English
  shares. A fraction rather than a count, because a statement locator resolves
  to roughly a tenth of what a section locator does, and an absolute threshold
  loosens by that factor without anything appearing to change. Under the count
  it previously used, `interior_H2_estimate` pointed at Evans' *wave equation*
  came back `Entailed`.

  **It does not separate true pairings from mismatches.** Measured in July 2026
  at statement granularity, over 8 true pairings and 7 deliberate mismatches
  from one textbook: true pairings score 0.086 to 0.189, cross-chapter
  mismatches 0.063, 0.074, 0.077 and **0.158**, within-chapter mismatches 0.14
  to 0.20. A declaration pointed at a wave-equation uniqueness theorem outscores
  five of the eight correct pairings. At section granularity the stub did refuse
  cross-chapter mismatches, 4 of 4; shrinking the operand tenfold removed even
  that, because a short passage shares too few tokens for overlap to carry
  signal. Read a stub verdict as evidence that the pipeline ran, never as
  evidence about a correspondence.
- `LlmEntailment`: two calls per warrant, one per direction, speaking the
  Anthropic Messages API directly under a schema-constrained reply. Gated behind
  `PROOFSENSE_ENABLE_LLM`, which is what keeps the test suite offline: no test
  sets that variable.

  Measured in July 2026 over thirteen pairings judged blind, labels withheld and
  ids re-keyed by hash so the ordering carried no signal. Six of six authored
  mismatches were refused as `divergent` at confidence 0.90 to 0.97, with no
  false positives. Five of the seven real pairings came back `decl_specialises`,
  sitting at 0.70 to 0.72 on the first direction against 0.90 and above for the
  mismatches, so the judge marks the subtle cases as subtle, which is what a
  trust rung needs. One of those five is a known misattribution, and it reported
  `decl_specialises` from the verdict itself, refusing the second direction on
  the remark that had previously been caught only by a human reading the source.
  The remaining two real pairings came back `divergent`; one is a warrant an
  earlier run had passed, and whether that is a real overclaim or a judge
  disagreement at the subtle end is open.

  That run reached a model of the intended class through a separate harness
  rather than through this code path, so what it measures is whether the four-way
  relation is separable. The wired request, its schema and its token budget have
  been checked by reading them against the API reference and have not yet been
  exercised against the live endpoint. The discrimination set is not in this
  repository and does not run in CI.

## Worked example

The fixtures check `EllipticPdes.Regularity.interior_H2_estimate`, the interior
H2 estimate from
[EllipticPDE](https://github.com/alejandro-soto-franco/EllipticPDE), against
Evans, *Partial Differential Equations*, section 6.3.1, the textbook result it
claims to formalise. The subject project is a git dependency of the Lean side, so
the example resolves from a clean clone.

## Status

Implemented and covered by tests: ingest, locator resolution, the Lean bridge,
deterministic verbalisation, both entailment backends, the two directional checks
and the relation derived from them, the warrant claim, rung and defect labelling,
the report wrapper, and the end-to-end orchestrator. 65 tests, all offline.

Three further tests pin the ingest against a real transcription. They are
ignored by default and read their input from the environment, since the sources
are copyrighted textbooks and stay out of this repository.

Not yet implemented: the `EntailedFormal` and `Discharged` rungs are defined in
the lattice and no code path produces them. Checking is statement-level, one
warrant at a time, and does not
descend into proof steps. The verbaliser's reading table covers core logical and
order constants through checked name literals, plus a set of Mathlib head symbols
matched by unchecked name literals, with everything else falling back to verbatim
pretty-printing.

Two gaps worth naming: reports serialise and match the published contract, and
nothing reads one back yet; and the live endpoint has never been called through
this code path.

## Licence

Apache-2.0.
