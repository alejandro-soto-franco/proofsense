//! Entailment backend: check whether a literature passage (the premise)
//! entails a machine-English rendering of a Lean declaration's statement
//! (the hypothesis).
//!
//! Two implementations:
//! - [`StubEntailment`]: deterministic, lexical-overlap heuristic. Purely
//!   a test double; its rationale always says "stub" and it is never
//!   exercised against a real model.
//! - [`LlmEntailment`]: a real NLI call to an LLM judge. Wired and
//!   compiles, but gated behind an env var so it is never invoked by
//!   `cargo test`; tests must run fully offline.

use crate::verdict::Judgement;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;

/// Pluggable entailment check: does `passage` (the premise) entail
/// `machine_english` (the hypothesis)? Returns the judgement, a short
/// human-readable rationale, and a confidence in `[0.0, 1.0]`.
pub trait Entailment {
    fn check(
        &self,
        passage: &str,
        machine_english: &str,
    ) -> anyhow::Result<(Judgement, String, f32)>;
}

/// A small, fixed set of English function words excluded from the salient
/// token count. Deliberately conservative: quantifiers like "every" and
/// "compact"/"set" as an ordinary noun stay in, since in this domain they
/// often carry meaning (e.g. "for every compact set").
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "then", "else", "for", "of", "in", "on", "at",
    "to", "by", "with", "from", "as", "is", "are", "was", "were", "be", "been", "being", "that",
    "this", "these", "those", "it", "its", "which", "who", "whom", "there", "here", "such", "not",
    "no", "so", "than", "too", "very", "can", "will", "would", "should", "could", "may", "might",
    "must", "shall", "do", "does", "did", "have", "has", "had",
];

/// Tokenize `s` into its salient-token set: lowercase, split on any
/// non-ASCII-alphanumeric character (so e.g. `"L^2"` yields `"l"` and
/// `"2"`, and punctuation/whitespace disappear), then drop stopwords.
/// Short domain tokens (single letters, digits) are kept: in this
/// domain's notation (norms, exponents, coefficient names) they carry
/// meaning, so filtering by length would discard real signal.
fn salient_tokens(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .filter(|t| !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

/// A deterministic test double for [`Entailment`]. It never calls out to
/// any model or network; it purely computes lexical overlap.
///
/// # Token rule
///
/// Both `passage` and `machine_english` are tokenized into their
/// [`salient_tokens`] sets (see that function's doc for the exact
/// normalisation). If the two sets share at least
/// `min_shared_salient_tokens` tokens, the result is
/// [`Judgement::Entailed`]; otherwise it is [`Judgement::Uncertain`] (the
/// stub has no way to detect contradiction, so it never returns
/// [`Judgement::NotEntailed`]; weak or absent lexical overlap is treated
/// as "can't tell", not "false"). Confidence is the fraction of the
/// hypothesis's salient tokens found in the passage, clamped to `1.0`.
///
/// The rationale always begins with `"stub:"`, since this is a test double, not
/// a real entailment check, and must never claim otherwise.
#[derive(Debug, Clone)]
pub struct StubEntailment {
    /// Minimum number of shared salient tokens for a verdict of
    /// [`Judgement::Entailed`]. Default (`3`) is chosen so that a genuinely
    /// related passage/claim pair, sharing a handful of technical terms,
    /// clears the bar, while two unrelated sentences do not.
    pub min_shared_salient_tokens: usize,
}

impl Default for StubEntailment {
    fn default() -> Self {
        Self {
            min_shared_salient_tokens: 3,
        }
    }
}

impl Entailment for StubEntailment {
    fn check(
        &self,
        passage: &str,
        machine_english: &str,
    ) -> anyhow::Result<(Judgement, String, f32)> {
        let passage_tokens = salient_tokens(passage);
        let hypothesis_tokens = salient_tokens(machine_english);

        let shared: Vec<&String> = hypothesis_tokens.intersection(&passage_tokens).collect();
        let shared_count = shared.len();

        let hypothesis_len = hypothesis_tokens.len().max(1);
        let confidence = (shared_count as f32 / hypothesis_len as f32).min(1.0);

        if shared_count >= self.min_shared_salient_tokens {
            Ok((
                Judgement::Entailed,
                format!(
                    "stub: {shared_count} shared salient token(s) meets threshold {} (deterministic lexical-overlap test double, not a real check)",
                    self.min_shared_salient_tokens
                ),
                confidence,
            ))
        } else {
            Ok((
                Judgement::Uncertain,
                format!(
                    "stub: only {shared_count} shared salient token(s), below threshold {} (deterministic lexical-overlap test double, not a real check)",
                    self.min_shared_salient_tokens
                ),
                confidence,
            ))
        }
    }
}

/// Env var that must be set (to any non-empty value) for [`LlmEntailment`]
/// to actually perform a network call. Absent this, `check` returns an
/// error immediately without touching the network, which is what keeps
/// `cargo test` offline even though `LlmEntailment` is wired and compiled.
const ENABLE_ENV_VAR: &str = "PROOFSENSE_ENABLE_LLM";

/// API key env var, read at construction time. Matches the Anthropic SDKs'
/// primary credential variable.
const API_KEY_ENV_VAR: &str = "ANTHROPIC_API_KEY";

/// Endpoint override env var; defaults to the standard Anthropic Messages
/// API endpoint when unset.
const ENDPOINT_ENV_VAR: &str = "PROOFSENSE_LLM_ENDPOINT";
const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// Model override env var; defaults to Claude Opus 4.8 when unset.
const MODEL_ENV_VAR: &str = "PROOFSENSE_LLM_MODEL";
const DEFAULT_MODEL: &str = "claude-opus-4-8";

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// A real NLI entailment backend, calling an LLM judge (default: the
/// Anthropic Messages API) with `passage` as the premise and
/// `machine_english` as the hypothesis.
///
/// Rust has no official Anthropic SDK, so this speaks the Messages API
/// directly over HTTP (`reqwest`, blocking). The call only happens when
/// [`ENABLE_ENV_VAR`] is set; otherwise [`LlmEntailment::check`] returns
/// an error without making a request. This is what keeps `cargo test`
/// fully offline: no test in this crate sets that env var, so this path
/// is wired and compiled but never exercised.
#[derive(Debug, Clone)]
pub struct LlmEntailment {
    /// Messages API endpoint URL.
    pub endpoint: String,
    /// Model id to request (e.g. `"claude-opus-4-8"`).
    pub model: String,
    /// API key, sent as the `x-api-key` header.
    pub api_key: String,
}

impl LlmEntailment {
    /// Build an [`LlmEntailment`] from the environment: `endpoint` from
    /// [`ENDPOINT_ENV_VAR`] (default [`DEFAULT_ENDPOINT`]), `model` from
    /// [`MODEL_ENV_VAR`] (default [`DEFAULT_MODEL`]), and `api_key` from
    /// [`API_KEY_ENV_VAR`] (required). This reads env vars but performs no
    /// network I/O.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = env::var(API_KEY_ENV_VAR)
            .with_context(|| format!("{API_KEY_ENV_VAR} is not set; required for LlmEntailment"))?;
        let endpoint = env::var(ENDPOINT_ENV_VAR).unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
        let model = env::var(MODEL_ENV_VAR).unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Ok(Self {
            endpoint,
            model,
            api_key,
        })
    }
}

/// Strict NLI prompt: instructs the judge to decide entailment between a
/// literature passage (premise) and a machine-English statement rendering
/// (hypothesis), and to reply with exactly one line of JSON so the
/// response is mechanically parseable.
fn build_prompt(passage: &str, machine_english: &str) -> String {
    format!(
        "You are a strict natural-language-inference judge for a proof-linting \
         tool. You are given a PREMISE (a transcribed passage from a literature \
         source) and a HYPOTHESIS (a machine-generated English rendering of a \
         formally verified mathematical statement). Decide whether the PREMISE \
         entails the HYPOTHESIS: does the passage support every claim the \
         hypothesis makes, with no unsupported strengthening.\n\n\
         Respond with EXACTLY one line of JSON and nothing else, matching this \
         shape: {{\"judgement\": \"entailed\" | \"not_entailed\" | \"uncertain\", \
         \"rationale\": \"<one sentence>\", \"confidence\": <number between 0 and 1>}}\n\n\
         PREMISE:\n{passage}\n\nHYPOTHESIS:\n{machine_english}\n"
    )
}

#[derive(Debug, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<MessageParam<'a>>,
}

#[derive(Debug, Serialize)]
struct MessageParam<'a> {
    role: &'a str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ResponseBlock>,
}

#[derive(Debug, Deserialize)]
struct ResponseBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

/// The structured reply we ask the judge to emit (see [`build_prompt`]).
#[derive(Debug, Deserialize)]
struct NliReply {
    judgement: String,
    rationale: String,
    confidence: f32,
}

fn parse_judgement(s: &str) -> anyhow::Result<Judgement> {
    match s {
        "entailed" => Ok(Judgement::Entailed),
        "not_entailed" => Ok(Judgement::NotEntailed),
        "uncertain" => Ok(Judgement::Uncertain),
        other => bail!("LlmEntailment: judge returned unrecognised judgement {other:?}"),
    }
}

impl Entailment for LlmEntailment {
    fn check(
        &self,
        passage: &str,
        machine_english: &str,
    ) -> anyhow::Result<(Judgement, String, f32)> {
        // Gate: never perform a network call unless explicitly enabled.
        // This is the whole reason `cargo test` stays offline with this
        // backend wired in, and no test sets this env var.
        if env::var(ENABLE_ENV_VAR)
            .map(|v| v.is_empty())
            .unwrap_or(true)
        {
            bail!(
                "LlmEntailment is disabled; set {ENABLE_ENV_VAR}=1 to allow real NLI calls \
                 (this gate exists so `cargo test` never hits the network)"
            );
        }

        let prompt = build_prompt(passage, machine_english);
        let request = MessagesRequest {
            model: &self.model,
            max_tokens: 1024,
            messages: vec![MessageParam {
                role: "user",
                content: prompt,
            }],
        };

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .context("LlmEntailment: request to the Messages API failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            bail!("LlmEntailment: API returned {status}: {body}");
        }

        let parsed: MessagesResponse = response
            .json()
            .context("LlmEntailment: failed to parse Messages API response JSON")?;

        let text_block = parsed
            .content
            .into_iter()
            .find(|b| b.block_type == "text")
            .context("LlmEntailment: response had no text content block")?;

        let reply: NliReply = serde_json::from_str(text_block.text.trim()).with_context(|| {
            format!(
                "LlmEntailment: judge reply was not the expected JSON: {}",
                text_block.text
            )
        })?;

        let judgement = parse_judgement(&reply.judgement)?;
        Ok((judgement, reply.rationale, reply.confidence))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salient_tokens_splits_and_strips_stopwords() {
        let toks = salient_tokens("the second weak derivatives exist in L^2 with the bound");
        assert!(toks.contains("second"));
        assert!(toks.contains("weak"));
        assert!(toks.contains("l"));
        assert!(toks.contains("2"));
        assert!(!toks.contains("the"));
        assert!(!toks.contains("in"));
        assert!(!toks.contains("with"));
    }

    #[test]
    fn stub_returns_uncertain_below_threshold() {
        let (j, r, _c) = StubEntailment::default()
            .check(
                "completely unrelated text about cats",
                "a statement about elliptic operators",
            )
            .unwrap();
        assert!(matches!(j, Judgement::Uncertain));
        assert!(r.starts_with("stub:"));
    }

    #[test]
    fn stub_never_calls_network_and_rationale_says_stub() {
        // Purely a sanity check that the rationale states plainly that it is a
        // test double, per the "never claim a real check" constraint.
        let (_, r, _) = StubEntailment::default().check("x", "y").unwrap();
        assert!(r.contains("stub"));
    }

    #[test]
    fn llm_entailment_check_is_gated_when_env_var_unset() {
        // Ensure the gate is off in this test process, then confirm the
        // call errors out before any network I/O would occur.
        // SAFETY: single-threaded test setup step, not read concurrently
        // with a racing writer within this process.
        unsafe {
            std::env::remove_var(ENABLE_ENV_VAR);
        }
        let backend = LlmEntailment {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: "unused-in-this-test".to_string(),
        };
        let err = backend.check("premise", "hypothesis").unwrap_err();
        assert!(err.to_string().contains(ENABLE_ENV_VAR));
    }
}
