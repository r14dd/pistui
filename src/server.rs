use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::cache::TraceCache;
use crate::explain::ExplainClient;

#[derive(Embed)]
#[folder = "static/"]
struct Assets;

pub struct AppState {
    pub cache: Arc<TraceCache>,
    pub env_explain: Option<ExplainClient>,
    pub langfuse_host: Option<String>,
    pub focus_trace: Option<String>,
}

pub fn router(cache: Arc<TraceCache>, env_explain: Option<ExplainClient>) -> Router {
    let state = Arc::new(AppState {
        cache,
        env_explain,
        langfuse_host: None,
        focus_trace: None,
    });
    router_with_state(state)
}

pub fn router_with_state(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/traces", get(list_traces))
        .route("/api/traces/{trace_id}", get(get_trace))
        .route("/api/traces/{trace_id}/export", get(export_trace))
        .route("/api/traces/{trace_id}/explain", post(explain_trace))
        .route("/api/sessions", get(list_sessions))
        .route("/api/config", get(config))
        .route("/api/refresh", post(refresh))
        .with_state(state)
        .fallback(static_handler)
}

async fn list_traces(State(state): State<Arc<AppState>>) -> Response {
    axum::Json(state.cache.traces().await).into_response()
}

async fn get_trace(State(state): State<Arc<AppState>>, Path(trace_id): Path<String>) -> Response {
    match state.cache.trace_detail(&trace_id).await {
        Ok((trace, tree, stats, scores)) => {
            let mut body = serde_json::json!({
                "trace": trace,
                "tree": tree,
                "stats": stats,
            });
            if !scores.is_empty() {
                body["scores"] = serde_json::json!(scores);
            }
            axum::Json(body).into_response()
        }
        Err(e) => error_response(&e),
    }
}

async fn export_trace(
    State(state): State<Arc<AppState>>,
    Path(trace_id): Path<String>,
) -> Response {
    match state.cache.trace_detail(&trace_id).await {
        Ok((trace, tree, stats, scores)) => {
            let mut body = serde_json::json!({
                "trace": trace,
                "tree": tree,
                "stats": stats,
            });
            if !scores.is_empty() {
                body["scores"] = serde_json::json!(scores);
            }
            let filename = format!("pistui-{trace_id}.json");
            (
                [
                    (header::CONTENT_TYPE, "application/json".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{filename}\""),
                    ),
                ],
                serde_json::to_string_pretty(&body).unwrap_or_default(),
            )
                .into_response()
        }
        Err(e) => error_response(&e),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummary {
    session_id: String,
    trace_count: usize,
    first_trace: String,
    last_trace: String,
    total_cost: f64,
    traces: Vec<String>,
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> Response {
    let traces = state.cache.traces().await;
    let mut groups: HashMap<String, Vec<&crate::model::Trace>> = HashMap::new();

    for trace in &traces {
        if let Some(sid) = &trace.session_id {
            groups.entry(sid.clone()).or_default().push(trace);
        }
    }

    let mut sessions: Vec<SessionSummary> = groups
        .into_iter()
        .map(|(session_id, mut group)| {
            group.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            let total_cost = group.iter().filter_map(|t| t.total_cost).sum();
            let trace_ids: Vec<String> = group.iter().map(|t| t.id.clone()).collect();
            SessionSummary {
                session_id,
                trace_count: group.len(),
                first_trace: group
                    .first()
                    .map(|t| t.timestamp.clone())
                    .unwrap_or_default(),
                last_trace: group
                    .last()
                    .map(|t| t.timestamp.clone())
                    .unwrap_or_default(),
                total_cost,
                traces: trace_ids,
            }
        })
        .collect();

    sessions.sort_by(|a, b| b.last_trace.cmp(&a.last_trace));
    axum::Json(sessions).into_response()
}

async fn config(State(state): State<Arc<AppState>>) -> Response {
    let mut cfg = serde_json::json!({
        "explain": state.env_explain.is_some(),
    });
    if let Some(host) = &state.langfuse_host {
        cfg["langfuseHost"] = serde_json::json!(host);
    }
    if let Some(trace_id) = &state.focus_trace {
        cfg["focusTrace"] = serde_json::json!(trace_id);
    }
    axum::Json(cfg).into_response()
}

#[derive(Deserialize)]
struct ExplainRequest {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

async fn explain_trace(
    State(state): State<Arc<AppState>>,
    Path(trace_id): Path<String>,
    body: Option<axum::Json<ExplainRequest>>,
) -> Response {
    let req = body.map(|b| b.0);
    let client_from_request = req.as_ref().and_then(|r| {
        let key = r.api_key.as_deref()?;
        let base = r.base_url.as_deref().unwrap_or("https://api.openai.com/v1");
        let model = r.model.as_deref().unwrap_or("gpt-4o-mini");
        Some(ExplainClient::new(key, base, model))
    });

    let client = client_from_request.as_ref().or(state.env_explain.as_ref());

    let Some(client) = client else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "no LLM configured - provide api_key in request or set OPENAI_API_KEY"
            })),
        )
            .into_response();
    };

    let (trace, tree, stats, _scores) = match state.cache.trace_detail(&trace_id).await {
        Ok(d) => d,
        Err(e) => return error_response(&e),
    };

    let user_prompt = req.as_ref().and_then(|r| r.prompt.clone());

    match client
        .explain(&trace, &tree, &stats, user_prompt.as_deref())
        .await
    {
        Ok(explanation) => {
            axum::Json(serde_json::json!({ "explanation": explanation })).into_response()
        }
        Err(e) => error_response(&e),
    }
}

async fn refresh(State(state): State<Arc<AppState>>) -> Response {
    match state.cache.refresh().await {
        Ok(()) => {
            let count = state.cache.traces().await.len();
            axum::Json(serde_json::json!({ "traces": count })).into_response()
        }
        Err(e) => error_response(&e),
    }
}

fn error_response(err: &crate::model::Error) -> Response {
    let (status, message) = match err {
        crate::model::Error::Auth => (StatusCode::UNAUTHORIZED, err.to_string()),
        crate::model::Error::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
        crate::model::Error::RateLimit => (StatusCode::TOO_MANY_REQUESTS, err.to_string()),
        crate::model::Error::Explain(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
        _ => (StatusCode::BAD_GATEWAY, err.to_string()),
    };
    (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => match Assets::get("index.html") {
            Some(index) => Html(String::from_utf8_lossy(&index.data).to_string()).into_response(),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}
