use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use pistui::cache::TraceCache;
use pistui::langfuse;
use pistui::server::{router, router_with_state, AppState};

async fn setup() -> (axum::Router, MockServer) {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    (router(cache, None), mock)
}

async fn setup_warmed() -> (axum::Router, MockServer, Arc<TraceCache>) {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let app = router(cache.clone(), None);
    (app, mock, cache)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn post(uri: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// --- static asset serving ---

#[tokio::test]
async fn root_serves_index_html() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(get("/")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/html"), "expected html, got {ct}");
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("<title>pistui</title>"));
}

#[tokio::test]
async fn css_served_with_correct_content_type() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(get("/style.css")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("css"), "expected css content-type, got {ct}");
}

#[tokio::test]
async fn js_served_with_correct_content_type() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(get("/app.js")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        ct.contains("javascript"),
        "expected js content-type, got {ct}"
    );
}

#[tokio::test]
async fn unknown_path_falls_back_to_index() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(get("/some/random/path")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("<title>pistui</title>"));
}

// --- GET /api/traces (served from cache) ---

#[tokio::test]
async fn list_traces_returns_cached_data() {
    let (app, mock, cache) = setup_warmed().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "t-1",
                    "name": "trace-one",
                    "timestamp": "2026-06-10T14:00:00.000Z",
                    "latency": 2.5,
                    "totalCost": 0.01,
                    "tags": ["prod"]
                },
                {
                    "id": "t-2",
                    "name": "trace-two",
                    "timestamp": "2026-06-10T15:00:00.000Z",
                    "tags": []
                }
            ]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let resp = app.oneshot(get("/api/traces")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let traces: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(traces.len(), 2);
    assert_eq!(traces[0]["id"], "t-1");
    assert_eq!(traces[1]["name"], "trace-two");
}

#[tokio::test]
async fn list_traces_returns_empty_before_warm() {
    let (app, _mock) = setup().await;

    let resp = app.oneshot(get("/api/traces")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let traces: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(traces.is_empty());
}

// --- GET /api/traces/{trace_id} ---

#[tokio::test]
async fn get_trace_returns_tree_and_stats() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-abc",
            "name": "my-trace",
            "timestamp": "2026-06-10T14:00:00.000Z"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-abc"))
        .and(query_param("limit", "100"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "obs-1",
                    "traceId": "t-abc",
                    "type": "SPAN",
                    "name": "root-span",
                    "startTime": "2026-06-10T14:00:00.000Z",
                    "endTime": "2026-06-10T14:00:02.000Z"
                },
                {
                    "id": "obs-2",
                    "traceId": "t-abc",
                    "type": "GENERATION",
                    "name": "llm-call",
                    "parentObservationId": "obs-1",
                    "startTime": "2026-06-10T14:00:00.500Z",
                    "endTime": "2026-06-10T14:00:01.500Z",
                    "model": "gpt-4o",
                    "usage": { "input": 200, "output": 100, "total": 300 },
                    "calculatedTotalCost": 0.005
                }
            ]
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-abc")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(data["trace"]["id"], "t-abc");
    assert_eq!(data["trace"]["name"], "my-trace");

    let tree = data["tree"].as_array().unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0]["name"], "root-span");
    assert_eq!(tree[0]["kind"], "SPAN");
    assert_eq!(tree[0]["durationMs"], 2000.0);

    let children = tree[0]["children"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["name"], "llm-call");
    assert_eq!(children[0]["kind"], "GENERATION");
    assert_eq!(children[0]["model"], "gpt-4o");
    assert_eq!(children[0]["tokensIn"], 200);
    assert_eq!(children[0]["tokensOut"], 100);
    assert_eq!(children[0]["cost"], 0.005);

    let stats = &data["stats"];
    assert_eq!(stats["spanCount"], 1);
    assert_eq!(stats["generationCount"], 1);
    assert_eq!(stats["totalTokensIn"], 200);
    assert_eq!(stats["totalTokensOut"], 100);
    assert_eq!(stats["totalCost"], 0.005);
    assert_eq!(stats["deepestNesting"], 2);
}

#[tokio::test]
async fn get_trace_with_no_observations() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-empty"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-empty",
            "timestamp": "2026-06-10T14:00:00.000Z"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-empty"))
        .and(query_param("limit", "100"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": []
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-empty"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-empty")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(data["tree"].as_array().unwrap().is_empty());
    assert_eq!(data["stats"]["spanCount"], 0);
}

// --- GET /api/traces/{id}/export ---

#[tokio::test]
async fn export_returns_json_attachment() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-exp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-exp", "name": "export-me", "timestamp": "2026-06-14T10:00:00.000Z"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-exp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-exp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-exp/export")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let cd = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cd.contains("pistui-t-exp.json"));

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["trace"]["id"], "t-exp");
}

// --- scores in trace detail ---

#[tokio::test]
async fn trace_detail_includes_scores() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-scored"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-scored", "timestamp": "2026-06-14T10:00:00.000Z"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-scored"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-scored"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                { "id": "s-1", "traceId": "t-scored", "name": "accuracy", "value": 0.9 },
                { "id": "s-2", "traceId": "t-scored", "name": "helpfulness", "value": 4.0 }
            ]
        })))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-scored")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let scores = data["scores"].as_array().unwrap();
    assert_eq!(scores.len(), 2);
    assert_eq!(scores[0]["name"], "accuracy");
    assert_eq!(scores[0]["value"], 0.9);
}

#[tokio::test]
async fn trace_detail_omits_scores_when_empty() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-no-scores"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-no-scores", "timestamp": "2026-06-14T10:00:00.000Z"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-no-scores"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-no-scores"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-no-scores")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(data.get("scores").is_none());
}

// --- model breakdown in stats ---

#[tokio::test]
async fn trace_detail_includes_model_breakdown() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-models", "timestamp": "2026-06-14T10:00:00.000Z"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "obs-1", "traceId": "t-models", "type": "GENERATION",
                    "name": "call-1", "startTime": "2026-06-14T10:00:00.000Z",
                    "endTime": "2026-06-14T10:00:01.000Z",
                    "model": "gpt-4o",
                    "usage": { "input": 100, "output": 50 },
                    "calculatedTotalCost": 0.005
                },
                {
                    "id": "obs-2", "traceId": "t-models", "type": "GENERATION",
                    "name": "call-2", "startTime": "2026-06-14T10:00:01.000Z",
                    "endTime": "2026-06-14T10:00:02.000Z",
                    "model": "gpt-4o-mini",
                    "usage": { "input": 200, "output": 100 },
                    "calculatedTotalCost": 0.001
                }
            ]
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-models")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let breakdown = data["stats"]["modelBreakdown"].as_array().unwrap();
    assert_eq!(breakdown.len(), 2);

    let gpt4o = breakdown.iter().find(|m| m["model"] == "gpt-4o").unwrap();
    assert_eq!(gpt4o["count"], 1);
    assert_eq!(gpt4o["tokensIn"], 100);
    assert_eq!(gpt4o["cost"], 0.005);
}

// --- GET /api/config ---

#[tokio::test]
async fn config_without_explain() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(get("/api/config")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["explain"], false);
}

#[tokio::test]
async fn config_with_explain() {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let explain = pistui::explain::ExplainClient::new("key", "http://localhost", "model").unwrap();
    let app = router(cache, Some(explain));

    let resp = app.oneshot(get("/api/config")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["explain"], true);
}

// --- POST /api/traces/{id}/explain ---

#[tokio::test]
async fn explain_with_request_key() {
    let langfuse_mock = MockServer::start().await;
    let llm_mock = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-1", "name": "test", "timestamp": "2026-06-14T10:00:00.000Z"
        })))
        .mount(&langfuse_mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-1"))
        .and(query_param("limit", "100"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": []
        })))
        .mount(&langfuse_mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&langfuse_mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "This trace did nothing." } }]
        })))
        .mount(&llm_mock)
        .await;

    let client = langfuse::Client::with_base_url(langfuse_mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let app = router(cache, None);

    let req = post_json(
        "/api/traces/t-1/explain",
        serde_json::json!({
            "api_key": "test-key",
            "base_url": llm_mock.uri(),
            "model": "gpt-4o-mini"
        }),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["explanation"], "This trace did nothing.");
}

#[tokio::test]
async fn explain_without_key_returns_400() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(post("/api/traces/t-1/explain")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(data["error"]
        .as_str()
        .unwrap()
        .contains("no LLM configured"));
}

// --- GET /api/config (extended) ---

#[tokio::test]
async fn config_returns_langfuse_host() {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let state = Arc::new(AppState {
        cache,
        env_explain: None,
        langfuse_host: Some("https://cloud.langfuse.com".into()),
        focus_trace: None,
    });
    let app = router_with_state(state);

    let resp = app.oneshot(get("/api/config")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["langfuseHost"], "https://cloud.langfuse.com");
}

#[tokio::test]
async fn config_returns_focus_trace() {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let state = Arc::new(AppState {
        cache,
        env_explain: None,
        langfuse_host: None,
        focus_trace: Some("t-focus-123".into()),
    });
    let app = router_with_state(state);

    let resp = app.oneshot(get("/api/config")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["focusTrace"], "t-focus-123");
}

#[tokio::test]
async fn config_omits_null_fields() {
    let (app, _mock) = setup().await;
    let resp = app.oneshot(get("/api/config")).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["explain"], false);
    assert!(data.get("langfuseHost").is_none() || data["langfuseHost"].is_null());
    assert!(data.get("focusTrace").is_none() || data["focusTrace"].is_null());
}

// --- GET /api/sessions ---

#[tokio::test]
async fn sessions_groups_traces_by_session_id() {
    let (app, mock, cache) = setup_warmed().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "t-1", "name": "step-1", "sessionId": "sess-a",
                    "timestamp": "2026-06-14T10:00:00.000Z", "latency": 1.0, "totalCost": 0.01
                },
                {
                    "id": "t-2", "name": "step-2", "sessionId": "sess-a",
                    "timestamp": "2026-06-14T10:01:00.000Z", "latency": 2.0, "totalCost": 0.02
                },
                {
                    "id": "t-3", "name": "solo", "sessionId": "sess-b",
                    "timestamp": "2026-06-14T11:00:00.000Z", "latency": 0.5, "totalCost": 0.005
                },
                {
                    "id": "t-4", "name": "no-session",
                    "timestamp": "2026-06-14T12:00:00.000Z"
                }
            ]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let resp = app.oneshot(get("/api/sessions")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let sessions: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(sessions.len(), 2);

    let sess_a = sessions
        .iter()
        .find(|s| s["sessionId"] == "sess-a")
        .unwrap();
    assert_eq!(sess_a["traceCount"], 2);
    assert_eq!(sess_a["totalCost"], 0.03);

    let sess_b = sessions
        .iter()
        .find(|s| s["sessionId"] == "sess-b")
        .unwrap();
    assert_eq!(sess_b["traceCount"], 1);
}

#[tokio::test]
async fn sessions_returns_empty_when_no_sessions() {
    let (app, mock, cache) = setup_warmed().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                { "id": "t-1", "name": "no-session", "timestamp": "2026-06-14T10:00:00.000Z" }
            ]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let resp = app.oneshot(get("/api/sessions")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let sessions: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(sessions.is_empty());
}

// --- POST /api/refresh ---

#[tokio::test]
async fn refresh_reloads_traces() {
    let (app, mock, cache) = setup_warmed().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                { "id": "t-1", "timestamp": "2026-06-10T14:00:00.000Z" },
                { "id": "t-2", "timestamp": "2026-06-10T15:00:00.000Z" }
            ]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let resp = app.oneshot(post("/api/refresh")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["traces"], 2);
}

// --- error propagation ---

#[tokio::test]
async fn langfuse_401_on_trace_detail() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/bad"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/bad")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(data["error"].as_str().unwrap().contains("authentication"));
}

#[tokio::test]
async fn langfuse_404_on_trace_detail() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/nonexistent"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/nonexistent")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn langfuse_500_on_refresh_returns_bad_gateway() {
    let (app, mock, _cache) = setup_warmed().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&mock)
        .await;

    let resp = app.oneshot(post("/api/refresh")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn langfuse_429_on_trace_detail_returns_too_many_requests() {
    let (app, mock) = setup().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-429"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&mock)
        .await;

    let resp = app.oneshot(get("/api/traces/t-429")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

async fn mount_trace_detail(mock: &MockServer, trace_id: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/api/public/traces/{trace_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": trace_id, "name": "test", "timestamp": "2026-06-14T10:00:00.000Z"
        })))
        .mount(mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", trace_id))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", trace_id))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(mock)
        .await;
}

#[tokio::test]
async fn explain_falls_back_to_env_client() {
    let langfuse_mock = MockServer::start().await;
    let llm_mock = MockServer::start().await;

    mount_trace_detail(&langfuse_mock, "t-1").await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "env explained." } }]
        })))
        .mount(&llm_mock)
        .await;

    let client = langfuse::Client::with_base_url(langfuse_mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let explain =
        pistui::explain::ExplainClient::new("env-key", &llm_mock.uri(), "gpt-4o-mini").unwrap();
    let app = router(cache, Some(explain));

    let resp = app.oneshot(post("/api/traces/t-1/explain")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(data["explanation"], "env explained.");
}

#[tokio::test]
async fn explain_forwards_custom_prompt() {
    let langfuse_mock = MockServer::start().await;
    let llm_mock = MockServer::start().await;

    mount_trace_detail(&langfuse_mock, "t-1").await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{ "message": { "content": "ok" } }]
        })))
        .mount(&llm_mock)
        .await;

    let client = langfuse::Client::with_base_url(langfuse_mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let app = router(cache, None);

    let req = post_json(
        "/api/traces/t-1/explain",
        serde_json::json!({
            "api_key": "k",
            "base_url": llm_mock.uri(),
            "prompt": "explain in French"
        }),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let received = llm_mock.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let body = String::from_utf8_lossy(&received[0].body);
    assert!(
        body.contains("explain in French"),
        "custom prompt not forwarded to LLM: {body}"
    );
}

#[tokio::test]
async fn explain_maps_llm_failure_to_bad_gateway() {
    let langfuse_mock = MockServer::start().await;
    let llm_mock = MockServer::start().await;

    mount_trace_detail(&langfuse_mock, "t-1").await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("model overloaded"))
        .mount(&llm_mock)
        .await;

    let client = langfuse::Client::with_base_url(langfuse_mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let app = router(cache, None);

    let req = post_json(
        "/api/traces/t-1/explain",
        serde_json::json!({
            "api_key": "k",
            "base_url": llm_mock.uri()
        }),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}
