use std::sync::Arc;

use tokio::net::TcpListener;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use pistui::cache::TraceCache;
use pistui::langfuse;
use pistui::server::router;

async fn start_server() -> (String, MockServer, Arc<TraceCache>) {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let app = router(cache.clone(), None);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (base, mock, cache)
}

#[tokio::test]
async fn full_flow_list_then_detail() {
    let (base, mock, cache) = start_server().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "t-1",
                    "name": "agent-run",
                    "timestamp": "2026-06-14T10:00:00.000Z",
                    "latency": 3.2,
                    "totalCost": 0.04,
                    "tags": ["staging"]
                },
                {
                    "id": "t-2",
                    "name": "chat",
                    "timestamp": "2026-06-14T11:00:00.000Z",
                    "tags": []
                }
            ]
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/t-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-1",
            "name": "agent-run",
            "timestamp": "2026-06-14T10:00:00.000Z",
            "input": { "prompt": "hello" },
            "output": { "response": "world" }
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/observations"))
        .and(query_param("traceId", "t-1"))
        .and(query_param("limit", "100"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "obs-root",
                    "traceId": "t-1",
                    "type": "SPAN",
                    "name": "orchestrator",
                    "startTime": "2026-06-14T10:00:00.000Z",
                    "endTime": "2026-06-14T10:00:03.200Z"
                },
                {
                    "id": "obs-gen",
                    "traceId": "t-1",
                    "type": "GENERATION",
                    "name": "gpt-4o-call",
                    "parentObservationId": "obs-root",
                    "startTime": "2026-06-14T10:00:00.100Z",
                    "endTime": "2026-06-14T10:00:02.100Z",
                    "model": "gpt-4o",
                    "usage": { "input": 500, "output": 300, "total": 800 },
                    "calculatedTotalCost": 0.012
                },
                {
                    "id": "obs-event",
                    "traceId": "t-1",
                    "type": "EVENT",
                    "name": "tool-call",
                    "parentObservationId": "obs-root",
                    "startTime": "2026-06-14T10:00:02.200Z",
                    "level": "ERROR",
                    "statusMessage": "timeout"
                }
            ]
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/public/scores"))
        .and(query_param("traceId", "t-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let http = reqwest::Client::new();

    let resp = http.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let html = resp.text().await.unwrap();
    assert!(html.contains("<title>pistui</title>"));
    assert!(html.contains("app.js"));

    let resp = http.get(format!("{base}/api/traces")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let traces: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(traces.len(), 2);
    assert_eq!(traces[0]["id"], "t-1");
    assert_eq!(traces[0]["tags"][0], "staging");

    let resp = http
        .get(format!("{base}/api/traces/t-1"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(detail["trace"]["name"], "agent-run");
    assert_eq!(detail["trace"]["input"]["prompt"], "hello");

    let tree = detail["tree"].as_array().unwrap();
    assert_eq!(tree.len(), 1);
    let root = &tree[0];
    assert_eq!(root["name"], "orchestrator");
    assert_eq!(root["kind"], "SPAN");
    assert_eq!(root["status"], "ok");
    assert_eq!(root["durationMs"], 3200.0);

    let children = root["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);

    assert_eq!(children[0]["name"], "gpt-4o-call");
    assert_eq!(children[0]["kind"], "GENERATION");
    assert_eq!(children[0]["model"], "gpt-4o");
    assert_eq!(children[0]["tokensIn"], 500);
    assert_eq!(children[0]["tokensOut"], 300);
    assert_eq!(children[0]["durationMs"], 2000.0);

    assert_eq!(children[1]["name"], "tool-call");
    assert_eq!(children[1]["kind"], "EVENT");
    assert_eq!(children[1]["status"], "error");
    assert_eq!(children[1]["statusMessage"], "timeout");

    let stats = &detail["stats"];
    assert_eq!(stats["spanCount"], 1);
    assert_eq!(stats["generationCount"], 1);
    assert_eq!(stats["eventCount"], 1);
    assert_eq!(stats["errorCount"], 1);
    assert_eq!(stats["totalTokensIn"], 500);
    assert_eq!(stats["totalTokensOut"], 300);
    assert_eq!(stats["totalCost"], 0.012);
    assert_eq!(stats["deepestNesting"], 2);
    assert_eq!(stats["totalDurationMs"], 3200.0);

    let resp = http.get(format!("{base}/style.css")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .contains("css"));

    let resp = http.get(format!("{base}/tree.js")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .contains("javascript"));

    let resp = http.get(format!("{base}/traces/t-1")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let html = resp.text().await.unwrap();
    assert!(html.contains("<title>pistui</title>"));
}

#[tokio::test]
async fn refresh_endpoint_reloads_cache() {
    let (base, mock, cache) = start_server().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{ "id": "t-1", "timestamp": "2026-06-14T10:00:00.000Z" }]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let http = reqwest::Client::new();

    let traces: Vec<serde_json::Value> = http
        .get(format!("{base}/api/traces"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(traces.len(), 1);

    let resp = http
        .post(format!("{base}/api/refresh"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["traces"], 1);
}

#[tokio::test]
async fn error_responses_are_json() {
    let (base, mock, _cache) = start_server().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces/bad"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad"))
        .mount(&mock)
        .await;

    let http = reqwest::Client::new();
    let resp = http
        .get(format!("{base}/api/traces/bad"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn concurrent_requests_handled() {
    let (base, mock, cache) = start_server().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{ "id": "t-1", "timestamp": "2026-06-14T10:00:00.000Z" }]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let http = reqwest::Client::new();
    let mut handles = Vec::new();

    for _ in 0..10 {
        let client = http.clone();
        let url = format!("{base}/api/traces");
        handles.push(tokio::spawn(async move {
            client.get(&url).send().await.unwrap().status()
        }));
    }

    for handle in handles {
        assert_eq!(handle.await.unwrap(), 200);
    }
}

#[tokio::test]
async fn config_endpoint_returns_explain_status() {
    let (base, _mock, _cache) = start_server().await;

    let http = reqwest::Client::new();
    let resp = http.get(format!("{base}/api/config")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["explain"], false);
}

#[tokio::test]
async fn sessions_endpoint_groups_by_session() {
    let (base, mock, cache) = start_server().await;

    Mock::given(method("GET"))
        .and(path("/api/public/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "t-1", "name": "step-1", "sessionId": "sess-x",
                    "timestamp": "2026-06-14T10:00:00.000Z", "totalCost": 0.01
                },
                {
                    "id": "t-2", "name": "step-2", "sessionId": "sess-x",
                    "timestamp": "2026-06-14T10:01:00.000Z", "totalCost": 0.02
                },
                {
                    "id": "t-3", "name": "solo",
                    "timestamp": "2026-06-14T11:00:00.000Z"
                }
            ]
        })))
        .mount(&mock)
        .await;

    cache.warm().await.unwrap();

    let http = reqwest::Client::new();
    let resp = http
        .get(format!("{base}/api/sessions"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let sessions: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["sessionId"], "sess-x");
    assert_eq!(sessions[0]["traceCount"], 2);
    assert_eq!(sessions[0]["totalCost"], 0.03);
}

#[tokio::test]
async fn config_with_langfuse_host() {
    let mock = MockServer::start().await;
    let client = langfuse::Client::with_base_url(mock.uri());
    let cache = Arc::new(TraceCache::new(client));
    let state = Arc::new(pistui::server::AppState {
        cache,
        env_explain: None,
        langfuse_host: Some("https://cloud.langfuse.com".into()),
        focus_trace: None,
    });
    let app = pistui::server::router_with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();
    let resp = http.get(format!("{base}/api/config")).send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["langfuseHost"], "https://cloud.langfuse.com");
}
