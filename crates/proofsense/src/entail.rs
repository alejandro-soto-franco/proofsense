//! Entailment backends: answer, separately, whether a literature passage
//! entails a machine-English rendering of a Lean declaration's statement,
//! and whether that rendering entails the passage. Neither backend
//! classifies the relation between the two answers; that derivation lives
//! in [`crate::verdict::Relation::derive`].
//!
//! Two implementations:
//! - [`StubEntailment`]: deterministic, lexical-overlap heuristic. Purely
//!   a test double; its rationale always says "stub" and it is never
//!   exercised against a real model.
//! - [`LlmEntailment`]: a real NLI call to an LLM judge. Wired and
//!   compiles, but gated behind an env var so it is never invoked by
//!   `cargo test`; tests must run fully offline.

use crate::verdict::Directional;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;

/// One directional entailment answer per direction: does `passage` entail
/// `machine_english`, and does `machine_english` entail `passage`. Both
/// answers come from the same backend, and the relation between them is
/// derived by [`crate::verdict::Relation::derive`].
pub struct RelationCheck {
    /// Does the source passage entail the machine English?
    pub source_entails_decl: Directional,
    /// Does the machine English entail the source passage?
    pub decl_entails_source: Directional,
}

/// Pluggable entailment check. Implementors answer both directions and
/// declare the confidence below which their own answers mean nothing.
pub trait Entailment {
    /// Answer both directions between `passage` and `machine_english`.
    fn check(&self, passage: &str, machine_english: &str) -> anyhow::Result<RelationCheck>;

    /// The confidence below which this backend's answers are treated as
    /// undecided. A backend whose confidence is not a calibrated probability
    /// returns `0.0`.
    fn confidence_floor(&self) -> f32;

    /// Describe this backend for the run report. The backend reports its own
    /// identity so a report can never claim a judge that did not run.
    fn describe(&self) -> crate::report::JudgeInfo;
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
/// `min_shared_salient_tokens` tokens, `holds` is `true` for that direction;
/// otherwise it is `false` (the stub has no way to detect contradiction
/// separately from absent overlap, so a weak or absent match reads as `false`,
/// not as a distinct "uncertain" state). Confidence is the fraction of the
/// hypothesis's salient tokens found in the passage, clamped to `1.0`.
///
/// The rationale always begins with `"stub:"`, since this is a test double, not
/// a real entailment check, and must never claim otherwise.
#[derive(Debug, Clone)]
pub struct StubEntailment {
    /// Minimum number of shared salient tokens for `holds` to be `true`.
    /// Default (`3`) is chosen so that a genuinely related passage/claim
    /// pair, sharing a handful of technical terms, clears the bar, while two
    /// unrelated sentences do not.
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
    fn check(&self, passage: &str, machine_english: &str) -> anyhow::Result<RelationCheck> {
        let passage_tokens = salient_tokens(passage);
        let hypothesis_tokens = salient_tokens(machine_english);

        let shared_count = hypothesis_tokens.intersection(&passage_tokens).count();
        let hypothesis_len = hypothesis_tokens.len().max(1);
        let confidence = (shared_count as f32 / hypothesis_len as f32).min(1.0);
        let holds = shared_count >= self.min_shared_salient_tokens;

        let rationale = format!(
            "stub: {shared_count} shared salient token(s) against threshold {} \
             (deterministic lexical-overlap test double, not a real check)",
            self.min_shared_salient_tokens
        );

        // Lexical overlap is symmetric, so the stub answers both directions
        // the same way and can never report a one-sided relation.
        Ok(RelationCheck {
            source_entails_decl: Directional {
                holds,
                confidence,
                rationale: rationale.clone(),
            },
            decl_entails_source: Directional {
                holds,
                confidence,
                rationale,
            },
        })
    }

    /// Zero: the stub's confidence is an overlap ratio, not a probability, so
    /// a shared floor would silently make every stub verdict undecided.
    fn confidence_floor(&self) -> f32 {
        0.0
    }

    fn describe(&self) -> crate::report::JudgeInfo {
        crate::report::JudgeInfo {
            kind: "stub",
            model: None,
            effort: None,
            confidence_floor: self.confidence_floor(),
            prompt_sha256: None,
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

/// Model override env var; defaults to Claude Opus 5 when unset.
const MODEL_ENV_VAR: &str = "PROOFSENSE_LLM_MODEL";
const DEFAULT_MODEL: &str = "claude-opus-5";

/// Effort override env var; defaults to the API's own default level.
const EFFORT_ENV_VAR: &str = "PROOFSENSE_LLM_EFFORT";
const DEFAULT_EFFORT: &str = "high";

/// Output budget. On Claude Opus 5 thinking runs by default and `max_tokens`
/// bounds thinking and response text together, so this has to leave room for
/// both. 16000 is the documented non-streaming default.
const MAX_TOKENS: u32 = 16000;

/// Default confidence floor for the LLM judge. Below this, a direction is
/// treated as undecided and the relation is `Undetermined`.
const DEFAULT_CONFIDENCE_FLOOR: f32 = 0.5;

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// A real NLI entailment backend, calling an LLM judge (default: the
/// Anthropic Messages API) with `passage` as the premise and
/// `machine_english` as the hypothesis.
///
/// Rust has no official Anthropic SDK, so this speaks the Messages API
/// directly over HTTP (`reqwest`, blocking). The call only happens when
/// [`ENABLE_ENV_VAR`] is set; otherwise each direction's request returns
/// an error without making a request. This is what keeps `cargo test`
/// fully offline: no test in this crate sets that env var, so this path
/// is wired and compiled but never exercised.
#[derive(Debug, Clone)]
pub struct LlmEntailment {
    /// Messages API endpoint URL.
    pub endpoint: String,
    /// Model id to request (e.g. `"claude-opus-5"`).
    pub model: String,
    /// API key, sent as the `x-api-key` header.
    pub api_key: String,
    /// Effort level, sent explicitly on every request so the recorded value is
    /// never inferred from an absent field.
    pub effort: String,
    /// The confidence below which this backend's answers are treated as
    /// undecided.
    pub confidence_floor: f32,
}

impl LlmEntailment {
    /// Build an [`LlmEntailment`] from the environment: `endpoint` from
    /// [`ENDPOINT_ENV_VAR`] (default [`DEFAULT_ENDPOINT`]), `model` from
    /// [`MODEL_ENV_VAR`] (default [`DEFAULT_MODEL`]), `effort` from
    /// [`EFFORT_ENV_VAR`] (default [`DEFAULT_EFFORT`]), and `api_key` from
    /// [`API_KEY_ENV_VAR`] (required). This reads env vars but performs no
    /// network I/O.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = env::var(API_KEY_ENV_VAR)
            .with_context(|| format!("{API_KEY_ENV_VAR} is not set; required for LlmEntailment"))?;
        let endpoint = env::var(ENDPOINT_ENV_VAR).unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
        let model = env::var(MODEL_ENV_VAR).unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let effort = env::var(EFFORT_ENV_VAR).unwrap_or_else(|_| DEFAULT_EFFORT.to_string());
        Ok(Self {
            endpoint,
            model,
            api_key,
            effort,
            confidence_floor: DEFAULT_CONFIDENCE_FLOOR,
        })
    }

    /// Ask one directional question: does `premise` entail `hypothesis`.
    fn ask(&self, premise: &str, hypothesis: &str) -> anyhow::Result<Directional> {
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

        let request = build_request(&self.model, &self.effort, premise, hypothesis);

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

        directional_from_response(parsed)
    }
}

/// The directional NLI prompt. One template, used for both directions by
/// swapping the operands, so a run has exactly one prompt hash to record.
const PROMPT_TEMPLATE: &str = "You are a strict natural-language-inference judge for a \
     proof-linting tool. You are given a PREMISE and a HYPOTHESIS. Decide whether the \
     PREMISE entails the HYPOTHESIS: does the premise support every claim the hypothesis \
     makes, with no unsupported strengthening. Answer only about this direction. Do not \
     consider whether the hypothesis entails the premise.\n\n\
     PREMISE:\n{premise}\n\nHYPOTHESIS:\n{hypothesis}\n";

/// SHA-256 of [`PROMPT_TEMPLATE`], recorded in every report. A prompt change
/// changes verdicts, so it has to be visible in the provenance.
pub static PROMPT_TEMPLATE_SHA256: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| crate::hash::sha256_hex(PROMPT_TEMPLATE.as_bytes()));

fn build_prompt(premise: &str, hypothesis: &str) -> String {
    PROMPT_TEMPLATE
        .replace("{premise}", premise)
        .replace("{hypothesis}", hypothesis)
}

/// The reply schema. Structured-output schemas do not support numerical
/// constraints, so `confidence` carries no bounds and is clamped on receipt.
fn reply_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "holds": { "type": "boolean" },
            "confidence": { "type": "number" },
            "rationale": { "type": "string" }
        },
        "required": ["holds", "confidence", "rationale"],
        "additionalProperties": false
    })
}

#[derive(Debug, Serialize)]
struct OutputFormat {
    #[serde(rename = "type")]
    format_type: &'static str,
    schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OutputConfig<'a> {
    effort: &'a str,
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    output_config: OutputConfig<'a>,
    messages: Vec<MessageParam<'a>>,
}

fn build_request<'a>(
    model: &'a str,
    effort: &'a str,
    premise: &str,
    hypothesis: &str,
) -> MessagesRequest<'a> {
    MessagesRequest {
        model,
        max_tokens: MAX_TOKENS,
        output_config: OutputConfig {
            effort,
            format: OutputFormat {
                format_type: "json_schema",
                schema: reply_schema(),
            },
        },
        messages: vec![MessageParam {
            role: "user",
            content: build_prompt(premise, hypothesis),
        }],
    }
}

#[derive(Debug, Serialize)]
struct MessageParam<'a> {
    role: &'a str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<ResponseBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    stop_details: Option<StopDetails>,
}

#[derive(Debug, Deserialize)]
struct StopDetails {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

/// The schema-constrained reply (see [`reply_schema`]).
#[derive(Debug, Deserialize)]
struct DirectionalReply {
    holds: bool,
    confidence: f32,
    rationale: String,
}

/// Read one directional answer out of a Messages API response.
///
/// A `stop_reason` of `refusal` is reported as a refusal carrying its
/// category, rather than surfacing as a missing text block.
fn directional_from_response(parsed: MessagesResponse) -> anyhow::Result<Directional> {
    if parsed.stop_reason.as_deref() == Some("refusal") {
        let category = parsed
            .stop_details
            .and_then(|d| d.category)
            .unwrap_or_else(|| "unspecified".to_string());
        bail!("LlmEntailment: the judge returned a refusal (category {category})");
    }

    let text_block = parsed
        .content
        .into_iter()
        .find(|b| b.block_type == "text")
        .context("LlmEntailment: response had no text content block")?;

    let reply: DirectionalReply =
        serde_json::from_str(text_block.text.trim()).with_context(|| {
            format!(
                "LlmEntailment: judge reply was not the expected JSON: {}",
                text_block.text
            )
        })?;

    Ok(Directional {
        holds: reply.holds,
        confidence: reply.confidence.clamp(0.0, 1.0),
        rationale: reply.rationale,
    })
}

impl Entailment for LlmEntailment {
    fn check(&self, passage: &str, machine_english: &str) -> anyhow::Result<RelationCheck> {
        Ok(RelationCheck {
            source_entails_decl: self.ask(passage, machine_english)?,
            decl_entails_source: self.ask(machine_english, passage)?,
        })
    }

    fn confidence_floor(&self) -> f32 {
        self.confidence_floor
    }

    fn describe(&self) -> crate::report::JudgeInfo {
        crate::report::JudgeInfo {
            kind: "llm",
            model: Some(self.model.clone()),
            effort: Some(self.effort.clone()),
            confidence_floor: self.confidence_floor,
            prompt_sha256: Some(PROMPT_TEMPLATE_SHA256.clone()),
        }
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
            effort: DEFAULT_EFFORT.to_string(),
            confidence_floor: DEFAULT_CONFIDENCE_FLOOR,
        };
        let err = backend.ask("premise", "hypothesis").unwrap_err();
        assert!(err.to_string().contains(ENABLE_ENV_VAR));
    }

    #[test]
    fn default_model_is_the_current_opus() {
        assert_eq!(DEFAULT_MODEL, "claude-opus-5");
    }

    /// On Claude Opus 5 thinking is on by default when the `thinking` field is
    /// omitted, and `max_tokens` caps thinking plus response text together. A
    /// 1024-token budget lets a hard discrimination spend the budget on
    /// thinking and truncate the reply, which then surfaces as a parse error.
    ///
    /// `MAX_TOKENS` is a compile-time constant, so this is deliberately an
    /// assertion on a constant: it is a regression guard against the budget
    /// being lowered back below the thinking-safe threshold.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn token_budget_leaves_room_for_thinking() {
        assert!(MAX_TOKENS >= 16000);
    }

    #[test]
    fn request_constrains_the_reply_with_a_schema() {
        let body = serde_json::to_value(build_request(
            "claude-opus-5",
            "high",
            "premise text",
            "hypothesis text",
        ))
        .unwrap();
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(
            body["output_config"]["format"]["schema"]["required"],
            serde_json::json!(["holds", "confidence", "rationale"])
        );
        assert_eq!(
            body["output_config"]["format"]["schema"]["additionalProperties"],
            serde_json::json!(false)
        );
        assert_eq!(body["output_config"]["effort"], "high");
        assert_eq!(body["max_tokens"], MAX_TOKENS);
    }

    /// Numerical constraints are not supported in structured-output schemas,
    /// so the schema carries no bounds on confidence and the client clamps.
    #[test]
    fn confidence_schema_carries_no_numeric_bounds() {
        let body = serde_json::to_value(build_request("m", "high", "p", "h")).unwrap();
        let confidence = &body["output_config"]["format"]["schema"]["properties"]["confidence"];
        assert_eq!(confidence["type"], "number");
        assert!(confidence.get("minimum").is_none());
        assert!(confidence.get("maximum").is_none());
    }

    #[test]
    fn a_refusal_is_reported_as_a_refusal() {
        let raw = r#"{"content":[],"stop_reason":"refusal","stop_details":{"category":"cyber"}}"#;
        let parsed: MessagesResponse = serde_json::from_str(raw).unwrap();
        let err = directional_from_response(parsed).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("refusal"), "{msg}");
        assert!(msg.contains("cyber"), "{msg}");
    }

    #[test]
    fn confidence_is_clamped_into_the_unit_interval() {
        let raw = r#"{"content":[{"type":"text","text":"{\"holds\":true,\"confidence\":1.4,\"rationale\":\"r\"}"}],"stop_reason":"end_turn"}"#;
        let parsed: MessagesResponse = serde_json::from_str(raw).unwrap();
        let d = directional_from_response(parsed).unwrap();
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn prompt_template_hash_is_stable_and_hex() {
        assert_eq!(PROMPT_TEMPLATE_SHA256.len(), 64);
        assert!(PROMPT_TEMPLATE_SHA256
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }
}
