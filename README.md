# proofsense

proofsense checks that a Lean 4 declaration states the theorem it cites, and
reports how the two stand to each other.

Formal verification establishes that a proof proves its statement. It
establishes nothing about whether that statement is the theorem the author says
it is, and that second link is where a formalisation goes wrong: a hypothesis
silently strengthened, a step citing a lemma the source never states, or an
inequality with the wrong direction. Lean answers "does this typecheck".
proofsense answers "is this the result the cited literature proves".

## Separating the generator from the judge

The obvious way to build this is to ask a model whether a Lean declaration
matches a paper. That fails for a reason no amount of prompting fixes: the model
produces both the reading of the Lean and the verdict. A misreading paired
with a confirming verdict is indistinguishable from a correct pair.

proofsense splits the two.

The English rendering of a declaration is produced by `verbalizeType : Expr →
MetaM String`, a deterministic fold over the declaration's own type expression
inside `MetaM`. One canonical reading per head symbol, drawn from a small trusted
reading table. Any subterm no rule matches is rendered verbatim through
`ppExpr`. The fold is therefore total: it degrades to less English, never to a different
claim. It is pure metaprogramming, with no network and no model involved.

The fold keeps every binder to which the statement refers. A `Prop` hypothesis
reads as an implication only when the body does not mention it; when the body
projects out of it, the hypothesis is named instead. No identifier appears in
the English without having been introduced.

The judge therefore never writes the claim. It only decides whether the source
supports a claim derived mechanically from the term under check. A wrong
judgement stays a wrong judgement; it cannot become a wrong reading of the Lean.

The judge is never asked for the relation directly. It answers one direction
at a time: does the passage entail the declaration, and, as a separate call,
does the declaration entail the passage. proofsense derives the relation from
the two answers. Asking for the relation directly lets a single call report
"matches" for a declaration that quietly assumes more than the source gives,
which is the case a binary verdict has no way to express.

## Pipeline

1. Ingest. Reviewable literature is OCR'd (MinerU) into a reading table: a
   structured, citable record of the source's definitions, hypotheses, and
   theorem statements. Both of MinerU's outputs are read, `content_list.json`
   and the Markdown export, dispatched by file extension. The Markdown export
   has no heading markers. Its structure is recovered from line form: a
   section heading needs a dotted number of at least two components, which keeps
   a numbered list item from opening a section.
2. Resolve. A warrant's locator is matched to a passage by string equality
   after normalisation. Resolution uses no embeddings and no fuzzy matching and is
   reproducible and inspectable. A locator names either a section (`§6.3.1`) or
   one statement inside it (`§6.3.1 Thm 1`).
3. Verbalise. The Lean declaration under check is rendered into English by
   the deterministic fold described above.
4. Relate. The rendered claim and the resolved passage are checked in both
   directions, and the relation between them is derived from the two answers.
5. Label. The relation is mapped, against what the warrant claims, to a rung
   on the trust ladder and an optional defect.

## Warrant claims and relations

A warrant names a declaration, a source and a locator, and declares what it
claims about the correspondence:

| `claim` | Reading |
|---|---|
| `formalises` | this declaration *is* the cited theorem |
| `follows_from` | this step is justified by the source |

The field defaults to `formalises`, the stricter reading. An unstated claim is
therefore never the lenient one.

The two directional answers give four relations, and a fifth for an answer below
the judge's confidence floor:

| Relation | Meaning |
|---|---|
| `equivalent` | each entails the other: the declaration states the cited result |
| `decl_specialises` | the source entails the declaration only: a special case of the cited result |
| `decl_exceeds` | the declaration entails the source only: it claims more than the source supports |
| `divergent` | neither direction is entailed |
| `undetermined` | a direction came back below the confidence floor |

The same relation classifies differently under each claim. That is why
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

## Trust ladder

Every warrant is labelled with exactly how strong its check was. A rung that
overstates what was checked is never assigned; this is a hard requirement.

| Rung | Meaning |
|---|---|
| `Bare` | A citation only. The locator resolved to no passage. |
| `Targeted` | The locator resolved and a relation was derived, and the relation falls short of what the warrant claims. A defect accompanies it in every case except `undetermined`. |
| `Entailed` | The derived relation supports the claim: `equivalent` under either claim, or `decl_specialises` under `follows_from`. |
| `EntailedFormal` | The passage was re-autoformalised and checked for equivalence. Still not unconditional: two-statement equivalence is undecidable in general. |
| `Discharged` | A Lean proof of the declaration exists, and the English is generated from a kernel-checked term. |

### Rungs currently reached

A locator that names a section resolves to a passage containing several theorems,
remarks and definitions, and the second direction then asks one declaration to
entail all of it. That is answered `false` almost always. The relation
therefore collapses to `decl_specialises` whenever the first direction holds. Across the
thirteen pairings described under `LlmEntailment` below, all judged against
section-granularity passages, `equivalent` came back zero times.

A locator may now name the statement it means, `§6.3.1 Thm 1` and not
`§6.3.1`, which removes the operand-scope reason those two relations were out of
reach. Measured over the nine warrants of a development checked against Evans,
section granularity hands the judge 88,249 characters where statement
granularity hands it 7,617 (a factor of 11.6). Section 6.3.1 alone runs to 12,555
characters over three theorems, and the theorem a declaration there formalises
is 1,304.

That has now been measured, and `equivalent` still came back zero times. The
reason is no longer operand scope at the section level.

Twelve pairings were judged blind in July 2026 at statement granularity, nine
real and three deliberate controls, twenty-four independent directional
judgements. All three controls were refused in both directions at confidence
0.94 to 0.99, while every real pairing's first direction sits at 0.60 to 0.91.
The two populations do not overlap. The four-way relation is therefore separable on
real operands, and the judge marks subtle cases as subtle.

The second direction came back false for all nine real pairings, and the
rationales agree on why: a numbered theorem in these sources is a conjunction of
several lettered parts, and a declaration typically formalises one of them. Asked
whether it entails the whole theorem, the answer is correctly no. **The operand
scope problem recurs one level down**, from section-versus-theorem to
theorem-versus-part. Until a locator can name `§6.2.3 Thm 4(iii)`, a faithful
formalisation of one conjunct cannot reach `equivalent` under `formalises`, and
`follows_from` remains the accurate claim for it, recording `decl_specialises` as
a clean pass at `Entailed`.

`decl_exceeds` also stayed unreached: no pairing produced a false first direction
with a true second.

`EntailedFormal` and `Discharged` remain defined and unreachable. No code path
produces either.

## Guarantees and limits

Verbalisation is faithful by construction: the English is a direct function of
the Lean declaration and cannot introduce claims the proof does not make.

The entailment check is evidence-grade, not a soundness proof. It reports what a
judge made of each direction, and either answer can be wrong. Deriving the
relation from two answers removes one failure mode, a single call reporting
"matches" for a declaration that assumes more than the source gives. The answers
themselves stay as reliable as the judge. proofsense flags mismatches for human
review. It does not certify correctness.

## Report

A run returns a `proofsense.report/1` document, not a list of verdicts.
It pins what was read and what judged it:

- the SHA-256 of the manifest bytes;
- the Lean toolchain, the subject package and its revision, and every package
  revision the Lake manifest pins;
- per source, the SHA-256 of the content-list bytes and the passage count;
- per verdict, the SHA-256 of the resolved passage text, the declaration's axiom
  dependencies, and whether its proof is free of `sorry`;
- the judge: kind, model, effort, confidence floor, and the SHA-256 of the prompt
  template, since a prompt change alters verdicts and has to be visible.

The judge block is filled in by whichever backend ran. A report therefore cannot
describe a judge that never ran. A stub run records no model, effort or prompt
hash at all.

Signing is out of scope. A detached signature over the document needs no schema
change.

## Trusted computing base

Results are only as good as three components proofsense does not itself verify:

- The reading table: its coverage, and its transcription of the source.
- MinerU OCR fidelity: the accuracy of the ingest step that produces the
  reading table from source documents.
- The entailment judge: the model or procedure deciding each direction.

A false negative or false positive in any of the three propagates into
proofsense's output.

## Requirements

`proofsense check` elaborates the Lean declaration by spawning `lake exe
proofsense-lean` from the Lake project under `lean/`. A Lean 4 toolchain and
that project must both be present, and a downloaded release binary has neither.

The Lake project requires the subject at a pinned revision, and **the pinned
revision sets what this tool reports**. Pinned one revision too early, the subject
contained an `interior_H2_estimate` with drift-free hypotheses its author had
already removed. A run would have verbalised a superseded statement and
re-reported findings the development had closed. Bump the pin whenever the
subject's statements move.

`--lean-info decl.json` is the one route that needs no Lean toolchain. It reads
a declaration record captured earlier by whoever did have Lean available, as
emitted by `lake exe proofsense-lean`.

`--stub` is a separate axis: it selects the deterministic lexical judge over the
LLM one. It removes the need for the network and the API key, and has no bearing on
whether Lean is spawned.

## Usage

A manifest names the sources, the Lean imports to elaborate under, and the
warrants (a declaration, the locator it cites, and what it claims about it).

```
# Offline run with the lexical stub judge
cargo run -- check path/to/manifest.json --stub

# Run with the LLM judge (both variables required; no request is made without them)
PROOFSENSE_ENABLE_LLM=1 ANTHROPIC_API_KEY=... \
  cargo run -- check path/to/manifest.json

# Skip spawning Lean by supplying a captured declaration record.
cargo run -- check path/to/manifest.json --stub --lean-info decl.json

# Check that every locator resolves (needs no Lean, judge or network)
cargo run -- resolve path/to/manifest.json
```

`resolve` reports the passage each locator matched, its size in characters and
its opening line, and exits non-zero on a miss. A warrant pointing at the wrong
passage yields a verdict about the wrong theorem, and no amount of judging
recovers from that. Run it on a manifest before any Lean or LLM step.

Entailment backends:

- `StubEntailment`: a deterministic lexical-overlap heuristic, and a test double
  and not a real check. It answers both directions identically by
  construction. It returns only `equivalent` or `divergent`, and the one-sided
  relations are invisible to it. Its rationale always begins with `stub:`, which distinguishes a
  stub verdict from a judged one.

  It scores the fraction of the passage's salient tokens the machine English
  shares. The score is a fraction and not a count, because a statement locator resolves
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
  that, because a short passage shares too few tokens for overlap to be
  a signal. Read a stub verdict as evidence that the pipeline ran, never as
  evidence about a correspondence.
- `LlmEntailment`: two calls per warrant, one per direction, speaking the
  Anthropic Messages API directly under a schema-constrained reply. It runs only when
  `PROOFSENSE_ENABLE_LLM` is set. No test sets that variable, which keeps the test suite offline.

  Measured in July 2026 over thirteen pairings judged blind, labels withheld and
  ids re-keyed by hash so the ordering gave no signal. Six of six authored
  mismatches were refused as `divergent` at confidence 0.90 to 0.97, with no
  false positives. Five of the seven real pairings came back `decl_specialises`,
  sitting at 0.70 to 0.72 on the first direction against 0.90 and above for the
  mismatches. The judge reports the subtle cases at lower confidence. One of those five is a known misattribution, and it reported
  `decl_specialises` from the verdict itself, refusing the second direction on
  the remark that had previously been caught only by a human reading the source.
  The remaining two real pairings came back `divergent`; one is a warrant an
  earlier run had passed, and whether that is a real overclaim or a judge
  disagreement at the subtle end is open.

  A second run in July 2026, after statement granularity landed, judged twelve
  pairings in both directions, nine real and three deliberate controls. All three
  controls were refused at 0.94 to 0.99 against real pairings at 0.60 to 0.91,
  with no overlap between the two populations. Six real pairings came back
  `decl_specialises` and three `divergent`. On the one warrant with a known
  divergence the judge named it unprompted, that the declaration quantifies its
  variable over a strictly smaller space than the cited theorem requires, and
  found a second weakening the earlier human audit had missed, that the estimate's
  constant is quantified after the data it is supposed to be independent of.

  Both runs reached a model of the intended class through a separate harness
  and not through this code path. They measure whether the four-way
  relation is separable. The wired request, its schema and its token limit have
  been checked by reading them against the API reference and have not yet been
  exercised against the live endpoint. Neither discrimination set is in this
  repository and neither runs in CI.

## Worked example

The fixtures check `EllipticPdes.Regularity.interior_H2_estimate`, the interior
H2 estimate from
[EllipticPDE](https://github.com/alejandro-soto-franco/EllipticPDE), against
Evans, *Partial Differential Equations*, section 6.3.1, the textbook result it
claims to formalise. The subject project is a git dependency of the Lean side.
The example therefore resolves from a clean clone.

## Status

Implemented and covered by tests: ingest, locator resolution, the Lean bridge,
deterministic verbalisation, both entailment backends, the two directional checks
and the relation derived from them, warrant claims with rung and defect labelling,
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

Two gaps remain: reports serialise and match the published contract, and
nothing reads one back yet; and the live endpoint has never been called through
this code path.

## Licence

Apache-2.0.
