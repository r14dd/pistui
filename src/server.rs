use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::Embed;
use serde::Deserialize;

use crate::cache::TraceCache;
use crate::explain::ExplainClient;

#[derive(Embed)]
#[folder = "static/"]
struct Assets;

pub struct AppState {
    pub cache: Arc<TraceCache>,
    pub env_explain: Option<ExplainClient>,
}

pub fn router(cache: Arc<TraceCache>, env_explain: Option<ExplainClient>) -> Router {
    let state = Arc::new(AppState { cache, env_explain });
    router_with_state(state)
}

pub fn router_with_state(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/traces", get(list_traces))
        .route("/api/traces/{trace_id}", get(get_trace))
        .route("/api/traces/{trace_id}/explain", post(explain_trace))
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
        Ok((trace, tree, stats)) => {
            let body = serde_json::json!({
                "trace": trace,
                "tree": tree,
                "stats": stats,
            });
            axum::Json(body).into_response()
        }
        Err(e) => error_response(&e),
    }
}

async fn config(State(state): State<Arc<AppState>>) -> Response {
    axum::Json(serde_json::json!({
        "explain": state.env_explain.is_some(),
    }))
    .into_response()
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
                "error": "no LLM configured — provide api_key in request or set OPENAI_API_KEY"
            })),
        )
            .into_response();
    };

    let (trace, tree, stats) = match state.cache.trace_detail(&trace_id).await {
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
