# proofsense

proofsense is a proof-linter: it checks that a Lean 4 proof corresponds to
the literature it claims to formalise. A Lean proof can typecheck and still
misrepresent its source — a hypothesis silently strengthened, a step that
cites a lemma the source never states, an inequality with the wrong
direction. proofsense targets that gap: not "does this proof typecheck"
(Lean already answers that) but "does this proof match what the ingested
paper actually says".

## Pipeline

1. **Ingest.** Reviewable literature (papers, textbooks) is OCR'd into a
   reading table: a structured, citable record of the source's definitions,
   hypotheses, and theorem statements.
2. **Faithful Lean→English.** Each Lean declaration under check is rendered
   into English by construction from its own syntax tree, not by a
   free-form paraphrase. The rendering is faithful because it is derived
   mechanically from the term proofsense is checking, not generated
   independently of it.
3. **Entailment check.** The rendered English claim is checked against the
   matching reading-table entry for entailment: does the source support
   what the proof claims.

## Honest guarantee

Generation (step 2) is faithful-by-construction: the English rendering is a
direct function of the Lean declaration, so it cannot introduce claims the
proof does not make. The entailment check (step 3) is evidence-grade, not a
soundness proof: it reports whether a judge finds the source consistent
with the rendered claim, and that judgment can be wrong. proofsense flags
mismatches for human review; it does not certify correctness.

## Trusted computing base

Results are only as good as three components proofsense does not itself
verify:

- **The reading table** — its coverage and its transcription of the source.
- **MinerU OCR fidelity** — the accuracy of the ingest step that produces
  the reading table from source documents.
- **The entailment judge** — the model or procedure that decides whether
  the reading table supports the rendered Lean claim.

A false negative or false positive in any of these three propagates into
proofsense's output.

## Status

Early scaffold. The CLI and Lean skeleton exist; ingest, rendering, and
entailment checking are not yet implemented.
