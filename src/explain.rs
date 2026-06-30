use serde::Deserialize;
use std::time::Duration;

use crate::model::{Error, Result, SpanNode, SpanStatus, Trace, TraceStats};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

// Maximum number of span nodes serialized into the explain prompt.
const MAX_PROMPT_NODES: usize = 60;

pub struct ExplainClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

impl ExplainClient {
    pub fn new(api_key: &str, base_url: &str, model: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("pistui")
            .build()
            .map_err(|e| Error::Explain(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        })
    }

    pub async fn explain(
        &self,
        trace: &Trace,
        tree: &[SpanNode],
        stats: &TraceStats,
        custom_prompt: Option<&str>,
    ) -> Result<String> {
        let mut user_prompt = build_prompt(trace, tree, stats);

        if let Some(custom) = custom_prompt {
            user_prompt.push_str("\n\nUser instructions: ");
            user_prompt.push_str(custom);
        }

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": SYSTEM_PROMPT,
                },
                {
                    "role": "user",
                    "content": user_prompt,
                }
            ],
            "temperature": 0.2
        });

        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.http.post(&url).json(&body);

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req
            .send()
            .await
            .map_err(|e| Error::Explain(format!("request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Explain(format!("LLM API returned {status}: {text}")));
        }

        let chat: ChatResponse = resp
            .json()
            .await
            .map_err(|e| Error::Explain(format!("failed to parse response: {e}")))?;

        chat.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| Error::Explain("empty response from LLM".into()))
    }
}

const SYSTEM_PROMPT: &str = "\
You are a concise debugging assistant analyzing LLM application traces. \
Explain what happened in plain language. Focus on: what the trace did, \
what went wrong (if anything), performance bottlenecks, and cost. \
Reference span names, models, and timings. 2-4 short paragraphs max.";

pub fn build_prompt(trace: &Trace, tree: &[SpanNode], stats: &TraceStats) -> String {
    let name = trace.name.as_deref().unwrap_or("(unnamed)");
    let duration = stats
        .total_duration_ms
        .map(|d| format!("{d:.0}ms"))
        .unwrap_or_else(|| "unknown".into());

    let mut out = format!(
        "Trace: {name}\n\
         Duration: {duration} | Tokens: {} in, {} out | Errors: {}\n\n\
         Span tree:\n",
        stats.total_tokens_in, stats.total_tokens_out, stats.error_count,
    );

    let mut count = 0usize;
    let mut pruned = 0usize;
    for node in tree {
        format_node(&mut out, node, 0, &mut count, &mut pruned);
    }

    if pruned > 0 {
        out.push_str(&format!("\n[{pruned} spans omitted for length]\n"));
    }

    out
}

fn format_node(
    out: &mut String,
    node: &SpanNode,
    depth: usize,
    count: &mut usize,
    pruned: &mut usize,
) {
    if *count >= MAX_PROMPT_NODES {
        *pruned += 1;
        // still count children so pruned total is accurate
        for child in &node.children {
            format_node(out, child, depth + 1, count, pruned);
        }
        return;
    }
    *count += 1;

    let indent = "  ".repeat(depth);
    let duration = node
        .duration_ms
        .map(|d| format!("{d:.0}ms"))
        .unwrap_or_default();
    let status = match node.status {
        SpanStatus::Ok => "ok",
        SpanStatus::Error => "ERROR",
    };

    out.push_str(&format!(
        "{indent}[{}] {} ({duration}, {status}",
        node.kind, node.name
    ));

    if let Some(model) = &node.model {
        out.push_str(&format!(", model: {model}"));
    }
    if let (Some(tin), Some(tout)) = (node.tokens_in, node.tokens_out) {
        out.push_str(&format!(", {tin}>{tout} tokens"));
    } else if let Some(total) = node.tokens_total {
        out.push_str(&format!(", {total} tokens"));
    }
    if let Some(ttft) = node.ttft_ms {
        out.push_str(&format!(", ttft: {ttft:.0}ms"));
    }
    out.push_str(")\n");

    if node.status == SpanStatus::Error {
        if let Some(msg) = &node.status_message {
            out.push_str(&format!("{indent}  error: {msg}\n"));
        }
    }

    for child in &node.children {
        format_node(out, child, depth + 1, count, pruned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SpanKind, SpanStatus, TraceStats};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_trace() -> Trace {
        Trace {
            id: "t-1".into(),
            name: Some("agent-run".into()),
            timestamp: "2026-06-14T10:00:00.000Z".into(),
            input: None,
            output: None,
            tags: vec![],
            user_id: None,
            session_id: None,
            release: None,
            latency: Some(3.2),
            total_cost: Some(0.012),
        }
    }

    fn sample_tree() -> Vec<SpanNode> {
        vec![SpanNode {
            id: "obs-root".into(),
            name: "orchestrator".into(),
            kind: SpanKind::Span,
            start_time: "2026-06-14T10:00:00.000Z".into(),
            end_time: Some("2026-06-14T10:00:03.200Z".into()),
            duration_ms: Some(3200.0),
            ttft_ms: None,
            status: SpanStatus::Ok,
            status_message: None,
            model: None,
            input: None,
            output: None,
            metadata: None,
            tokens_in: None,
            tokens_out: None,
            tokens_total: None,
            cost: None,
            scores: vec![],
            children: vec![
                SpanNode {
                    id: "obs-gen".into(),
                    name: "gpt-4o-call".into(),
                    kind: SpanKind::Generation,
                    start_time: "2026-06-14T10:00:00.100Z".into(),
                    end_time: Some("2026-06-14T10:00:02.100Z".into()),
                    duration_ms: Some(2000.0),
                    ttft_ms: Some(300.0),
                    status: SpanStatus::Ok,
                    status_message: None,
                    model: Some("gpt-4o".into()),
                    input: None,
                    output: None,
                    metadata: None,
                    tokens_in: Some(500),
                    tokens_out: Some(300),
                    tokens_total: Some(800),
                    cost: Some(0.012),
                    scores: vec![],
                    children: vec![],
                },
                SpanNode {
                    id: "obs-event".into(),
                    name: "tool-call".into(),
                    kind: SpanKind::Event,
                    start_time: "2026-06-14T10:00:02.200Z".into(),
                    end_time: None,
                    duration_ms: None,
                    ttft_ms: None,
                    status: SpanStatus::Error,
                    status_message: Some("timeout".into()),
                    model: None,
                    input: None,
                    output: None,
                    metadata: None,
                    tokens_in: None,
                    tokens_out: None,
                    tokens_total: None,
                    cost: None,
                    scores: vec![],
                    children: vec![],
                },
            ],
        }]
    }

    fn sample_stats() -> TraceStats {
        TraceStats {
            total_duration_ms: Some(3200.0),
            total_tokens_in: 500,
            total_tokens_out: 300,
            total_cost: 0.012,
            span_count: 1,
            generation_count: 1,
            event_count: 1,
            error_count: 1,
            deepest_nesting: 2,
            orphan_count: 0,
            truncated: false,
            model_breakdown: Vec::new(),
        }
    }

    #[test]
    fn prompt_includes_trace_name() {
        let prompt = build_prompt(&sample_trace(), &sample_tree(), &sample_stats());
        assert!(prompt.contains("Trace: agent-run"));
    }

    #[test]
    fn prompt_includes_stats() {
        let prompt = build_prompt(&sample_trace(), &sample_tree(), &sample_stats());
        assert!(prompt.contains("3200ms"));
        assert!(prompt.contains("500 in"));
        assert!(prompt.contains("300 out"));
        assert!(prompt.contains("Errors: 1"));
    }

    #[test]
    fn prompt_formats_nested_tree() {
        let prompt = build_prompt(&sample_trace(), &sample_tree(), &sample_stats());
        assert!(prompt.contains("[SPAN] orchestrator"));
        assert!(prompt.contains("  [GENERATION] gpt-4o-call"));
        assert!(prompt.contains("model: gpt-4o"));
        assert!(prompt.contains("500>300 tokens"));
        assert!(prompt.contains("  [EVENT] tool-call"));
    }

    #[test]
    fn prompt_includes_error_messages() {
        let prompt = build_prompt(&sample_trace(), &sample_tree(), &sample_stats());
        assert!(prompt.contains("error: timeout"));
    }

    #[test]
    fn prompt_includes_ttft() {
        let prompt = build_prompt(&sample_trace(), &sample_tree(), &sample_stats());
        assert!(prompt.contains("ttft: 300ms"));
    }

    #[test]
    fn prompt_with_custom_instructions() {
        let mut prompt = build_prompt(&sample_trace(), &sample_tree(), &sample_stats());
        prompt.push_str("\n\nUser instructions: explain in Turkish");
        assert!(prompt.contains("User instructions: explain in Turkish"));
        assert!(prompt.contains("Trace: agent-run"));
    }

    #[test]
    fn prompt_handles_unnamed_trace() {
        let mut trace = sample_trace();
        trace.name = None;
        let prompt = build_prompt(&trace, &[], &sample_stats());
        assert!(prompt.contains("Trace: (unnamed)"));
    }

    #[test]
    fn prompt_prunes_beyond_node_limit() {
        // Build a flat list of more than MAX_PROMPT_NODES nodes
        let mut tree = Vec::new();
        for i in 0..80 {
            tree.push(SpanNode {
                id: format!("n-{i}"),
                name: format!("span-{i}"),
                kind: SpanKind::Span,
                start_time: "2026-06-14T10:00:00.000Z".into(),
                end_time: None,
                duration_ms: None,
                ttft_ms: None,
                status: SpanStatus::Ok,
                status_message: None,
                model: None,
                input: None,
                output: None,
                metadata: None,
                tokens_in: None,
                tokens_out: None,
                tokens_total: None,
                cost: None,
                scores: vec![],
                children: vec![],
            });
        }
        let prompt = build_prompt(&sample_trace(), &tree, &sample_stats());
        assert!(prompt.contains("spans omitted"));
    }

    #[tokio::test]
    async fn explain_sends_request_and_returns_content() {
        let mock = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "This trace ran an orchestrator span."
                    }
                }]
            })))
            .expect(1)
            .mount(&mock)
            .await;

        let client = ExplainClient::new("test-key", &mock.uri(), "gpt-4o-mini").unwrap();
        let result = client
            .explain(&sample_trace(), &sample_tree(), &sample_stats(), None)
            .await
            .unwrap();

        assert_eq!(result, "This trace ran an orchestrator span.");
    }

    #[tokio::test]
    async fn explain_returns_error_on_non_200() {
        let mock = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid key"))
            .mount(&mock)
            .await;

        let client = ExplainClient::new("bad-key", &mock.uri(), "gpt-4o-mini").unwrap();
        let err = client
            .explain(&sample_trace(), &sample_tree(), &sample_stats(), None)
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Explain(_)));
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn explain_returns_error_on_empty_choices() {
        let mock = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "choices": [] })),
            )
            .mount(&mock)
            .await;

        let client = ExplainClient::new("key", &mock.uri(), "gpt-4o-mini").unwrap();
        let err = client
            .explain(&sample_trace(), &sample_tree(), &sample_stats(), None)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("empty response"));
    }

    #[tokio::test]
    async fn explain_sends_without_auth_header_when_key_empty() {
        let mock = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "ok" } }]
            })))
            .expect(1)
            .mount(&mock)
            .await;

        // Empty key = Ollama-style keyless local provider
        let client = ExplainClient::new("", &mock.uri(), "llama3.2").unwrap();
        let result = client
            .explain(&sample_trace(), &sample_tree(), &sample_stats(), None)
            .await
            .unwrap();
        assert_eq!(result, "ok");
    }
}
