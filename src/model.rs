use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpanKind {
    Span,
    Generation,
    Event,
}

impl fmt::Display for SpanKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Span => write!(f, "SPAN"),
            Self::Generation => write!(f, "GENERATION"),
            Self::Event => write!(f, "EVENT"),
        }
    }
}

// --- Langfuse API response types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub timestamp: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub latency: Option<f64>,
    #[serde(default)]
    pub total_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub id: String,
    pub trace_id: String,
    #[serde(rename = "type")]
    pub kind: SpanKind,
    #[serde(default)]
    pub name: Option<String>,
    pub start_time: String,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub completion_start_time: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub status_message: Option<String>,
    #[serde(default)]
    pub parent_observation_id: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub calculated_total_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub unit: Option<String>,
}

// --- Domain types (served to frontend) ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanNode {
    pub id: String,
    pub name: String,
    pub kind: SpanKind,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_ms: Option<f64>,
    pub ttft_ms: Option<f64>,
    pub status: SpanStatus,
    pub status_message: Option<String>,
    pub model: Option<String>,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub tokens_total: Option<u64>,
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scores: Vec<Score>,
    pub children: Vec<SpanNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStats {
    pub total_duration_ms: Option<f64>,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cost: f64,
    pub span_count: usize,
    pub generation_count: usize,
    pub event_count: usize,
    pub error_count: usize,
    pub deepest_nesting: usize,
    pub orphan_count: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub model_breakdown: Vec<ModelStats>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    pub model: String,
    pub count: usize,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub id: String,
    pub trace_id: String,
    pub name: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub string_value: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub observation_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub data_type: Option<String>,
}

// --- Errors ---

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to parse response: {0}")]
    Parse(String),

    #[error("authentication failed (check your Langfuse keys)")]
    Auth,

    #[error("trace not found: {0}")]
    NotFound(String),

    #[error("rate limited - try again shortly")]
    RateLimit,

    #[error("upstream error: {0}")]
    Upstream(String),

    #[error("explanation failed: {0}")]
    Explain(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_kind_display() {
        assert_eq!(SpanKind::Span.to_string(), "SPAN");
        assert_eq!(SpanKind::Generation.to_string(), "GENERATION");
        assert_eq!(SpanKind::Event.to_string(), "EVENT");
    }

    #[test]
    fn span_kind_roundtrips_through_json() {
        for kind in [SpanKind::Span, SpanKind::Generation, SpanKind::Event] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: SpanKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn span_kind_deserializes_from_langfuse_format() {
        assert_eq!(
            serde_json::from_str::<SpanKind>(r#""SPAN""#).unwrap(),
            SpanKind::Span
        );
        assert_eq!(
            serde_json::from_str::<SpanKind>(r#""GENERATION""#).unwrap(),
            SpanKind::Generation
        );
        assert_eq!(
            serde_json::from_str::<SpanKind>(r#""EVENT""#).unwrap(),
            SpanKind::Event
        );
    }

    #[test]
    fn score_deserializes_from_langfuse_response() {
        let json = r#"{
            "id": "s-1",
            "traceId": "t-1",
            "name": "user-feedback",
            "value": 1.0,
            "comment": "great",
            "source": "API"
        }"#;
        let score: Score = serde_json::from_str(json).unwrap();
        assert_eq!(score.id, "s-1");
        assert_eq!(score.name, "user-feedback");
        assert_eq!(score.value, Some(1.0));
        assert_eq!(score.comment.as_deref(), Some("great"));
    }

    #[test]
    fn score_deserializes_string_value() {
        let json = r#"{
            "id": "s-2",
            "traceId": "t-1",
            "name": "category",
            "stringValue": "positive",
            "dataType": "CATEGORICAL"
        }"#;
        let score: Score = serde_json::from_str(json).unwrap();
        assert_eq!(score.string_value.as_deref(), Some("positive"));
        assert_eq!(score.data_type.as_deref(), Some("CATEGORICAL"));
    }

    #[test]
    fn observation_deserializes_type_field_as_kind() {
        let json = r#"{
            "id": "obs-1",
            "traceId": "trace-1",
            "type": "GENERATION",
            "startTime": "2026-06-10T14:23:01.000Z"
        }"#;
        let obs: Observation = serde_json::from_str(json).unwrap();
        assert_eq!(obs.kind, SpanKind::Generation);
        assert_eq!(obs.trace_id, "trace-1");
        assert!(obs.parent_observation_id.is_none());
    }
}
