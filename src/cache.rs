use std::collections::HashMap;

use tokio::sync::RwLock;

use crate::langfuse;
use crate::model::{Result, SpanNode, Trace, TraceStats};
use crate::parser;

struct CacheState {
    traces: Vec<Trace>,
    details: HashMap<String, CachedDetail>,
}

#[derive(Clone)]
struct CachedDetail {
    trace: Trace,
    tree: Vec<SpanNode>,
    stats: TraceStats,
}

pub struct TraceCache {
    client: langfuse::Client,
    state: RwLock<CacheState>,
}

impl TraceCache {
    pub fn new(client: langfuse::Client) -> Self {
        Self {
            client,
            state: RwLock::new(CacheState {
                traces: Vec::new(),
                details: HashMap::new(),
            }),
        }
    }

    pub async fn warm(&self) -> Result<()> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let batch = self.client.list_traces(100, page).await?;
            let count = batch.len();
            all.extend(batch);
            if count < 100 {
                break;
            }
            page += 1;
        }
        let mut state = self.state.write().await;
        state.traces = all;
        state.details.clear();
        Ok(())
    }

    pub async fn traces(&self) -> Vec<Trace> {
        self.state.read().await.traces.clone()
    }

    pub async fn trace_detail(&self, trace_id: &str) -> Result<(Trace, Vec<SpanNode>, TraceStats)> {
        {
            let state = self.state.read().await;
            if let Some(d) = state.details.get(trace_id) {
                return Ok((d.trace.clone(), d.tree.clone(), d.stats.clone()));
            }
        }

        let trace = self.client.get_trace(trace_id).await?;
        let observations = self.client.get_observations(trace_id).await?;
        let (tree, stats) = parser::build_tree(&observations);

        let detail = CachedDetail {
            trace: trace.clone(),
            tree: tree.clone(),
            stats: stats.clone(),
        };
        self.state
            .write()
            .await
            .details
            .insert(trace_id.to_string(), detail);

        Ok((trace, tree, stats))
    }

    pub async fn refresh(&self) -> Result<()> {
        self.warm().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn setup() -> (TraceCache, MockServer) {
        let mock = MockServer::start().await;
        let client = langfuse::Client::with_base_url(mock.uri());
        (TraceCache::new(client), mock)
    }

    fn traces_json(traces: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "data": traces })
    }

    fn observations_json(obs: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "data": obs })
    }

    #[tokio::test]
    async fn traces_empty_before_warm() {
        let (cache, _mock) = setup().await;
        assert!(cache.traces().await.is_empty());
    }

    #[tokio::test]
    async fn warm_populates_trace_list() {
        let (cache, mock) = setup().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .and(query_param("limit", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(traces_json(
                serde_json::json!([
                    { "id": "t-1", "name": "alpha", "timestamp": "2026-06-14T10:00:00.000Z" },
                    { "id": "t-2", "name": "beta", "timestamp": "2026-06-14T11:00:00.000Z" }
                ]),
            )))
            .expect(1)
            .mount(&mock)
            .await;

        cache.warm().await.unwrap();
        let traces = cache.traces().await;
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[0].id, "t-1");
        assert_eq!(traces[1].id, "t-2");
    }

    #[tokio::test]
    async fn traces_served_from_cache_after_warm() {
        let (cache, mock) = setup().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(traces_json(
                serde_json::json!([
                    { "id": "t-1", "timestamp": "2026-06-14T10:00:00.000Z" }
                ]),
            )))
            .expect(1) // only one HTTP call, even though we read twice
            .mount(&mock)
            .await;

        cache.warm().await.unwrap();
        let first = cache.traces().await;
        let second = cache.traces().await;
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
    }

    #[tokio::test]
    async fn trace_detail_fetches_on_miss() {
        let (cache, mock) = setup().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces/t-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "t-1", "name": "my-trace", "timestamp": "2026-06-14T10:00:00.000Z"
            })))
            .expect(1)
            .mount(&mock)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/observations"))
            .and(query_param("traceId", "t-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(observations_json(
                serde_json::json!([{
                    "id": "obs-1",
                    "traceId": "t-1",
                    "type": "SPAN",
                    "name": "root",
                    "startTime": "2026-06-14T10:00:00.000Z",
                    "endTime": "2026-06-14T10:00:01.000Z"
                }]),
            )))
            .expect(1)
            .mount(&mock)
            .await;

        let (trace, tree, stats) = cache.trace_detail("t-1").await.unwrap();
        assert_eq!(trace.name.as_deref(), Some("my-trace"));
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "root");
        assert_eq!(stats.span_count, 1);
    }

    #[tokio::test]
    async fn trace_detail_serves_from_cache_on_hit() {
        let (cache, mock) = setup().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces/t-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "t-1", "timestamp": "2026-06-14T10:00:00.000Z"
            })))
            .expect(1) // exactly one fetch
            .mount(&mock)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/observations"))
            .and(query_param("traceId", "t-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(observations_json(serde_json::json!([]))),
            )
            .expect(1) // exactly one fetch
            .mount(&mock)
            .await;

        cache.trace_detail("t-1").await.unwrap();
        let (_, tree, _) = cache.trace_detail("t-1").await.unwrap();
        assert!(tree.is_empty());
    }

    #[tokio::test]
    async fn refresh_replaces_trace_list() {
        let (cache, mock) = setup().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(traces_json(
                serde_json::json!([
                    { "id": "t-old", "timestamp": "2026-06-14T10:00:00.000Z" }
                ]),
            )))
            .mount(&mock)
            .await;

        cache.warm().await.unwrap();
        assert_eq!(cache.traces().await[0].id, "t-old");

        mock.reset().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(traces_json(
                serde_json::json!([
                    { "id": "t-new", "timestamp": "2026-06-14T12:00:00.000Z" }
                ]),
            )))
            .mount(&mock)
            .await;

        cache.refresh().await.unwrap();
        assert_eq!(cache.traces().await[0].id, "t-new");
    }

    #[tokio::test]
    async fn refresh_clears_detail_cache() {
        let (cache, mock) = setup().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(traces_json(serde_json::json!([]))),
            )
            .mount(&mock)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces/t-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "t-1", "timestamp": "2026-06-14T10:00:00.000Z"
            })))
            .mount(&mock)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/observations"))
            .and(query_param("traceId", "t-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(observations_json(serde_json::json!([]))),
            )
            .mount(&mock)
            .await;

        cache.trace_detail("t-1").await.unwrap();
        cache.refresh().await.unwrap();

        // after refresh, detail should be re-fetched (not from stale cache)
        // the mock is still mounted, so it will serve the same data
        // but the point is: the cache was cleared, forcing a new fetch
        let received = mock.received_requests().await.unwrap();
        let detail_calls = received
            .iter()
            .filter(|r| r.url.path() == "/api/public/traces/t-1")
            .count();
        assert_eq!(detail_calls, 1);

        // now fetching again should hit Langfuse again
        cache.trace_detail("t-1").await.unwrap();
        let received = mock.received_requests().await.unwrap();
        let detail_calls = received
            .iter()
            .filter(|r| r.url.path() == "/api/public/traces/t-1")
            .count();
        assert_eq!(detail_calls, 2);
    }

    #[tokio::test]
    async fn warm_paginates_large_trace_lists() {
        let (cache, mock) = setup().await;

        let page1: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "id": format!("t-{i}"),
                    "timestamp": "2026-06-14T10:00:00.000Z"
                })
            })
            .collect();

        let page2 = vec![serde_json::json!({
            "id": "t-100",
            "timestamp": "2026-06-14T10:00:00.000Z"
        })];

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .and(query_param("page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": page1 })),
            )
            .expect(1)
            .mount(&mock)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .and(query_param("page", "2"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": page2 })),
            )
            .expect(1)
            .mount(&mock)
            .await;

        cache.warm().await.unwrap();
        let traces = cache.traces().await;
        assert_eq!(traces.len(), 101);
        assert_eq!(traces[100].id, "t-100");
    }
}
