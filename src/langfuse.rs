use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::time::Duration;

use crate::model::{Error, Observation, Result, Score, Trace};

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    max_retries: u32,
    base_delay: Duration,
}

const MAX_RETRIES: u32 = 3;
const BASE_DELAY: Duration = Duration::from_millis(300);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(serde::Deserialize)]
struct PagedResponse<T> {
    data: Vec<T>,
}

impl Client {
    pub fn new(host: &str, public_key: &str, secret_key: &str) -> Result<Self> {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("{public_key}:{secret_key}"));
        let mut auth_value = HeaderValue::from_str(&format!("Basic {credentials}"))
            .map_err(|e| Error::Parse(e.to_string()))?;
        auth_value.set_sensitive(true);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth_value);

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .default_headers(headers)
            .user_agent(concat!(
                "pistui/",
                env!("CARGO_PKG_VERSION"),
                " (trace viewer; https://github.com/r14dd/pistui)"
            ))
            .build()?;

        let base_url = host.trim_end_matches('/').to_string();
        Ok(Self {
            http,
            base_url,
            max_retries: MAX_RETRIES,
            base_delay: BASE_DELAY,
        })
    }

    #[doc(hidden)]
    pub fn with_base_url(base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            max_retries: MAX_RETRIES,
            base_delay: Duration::ZERO,
        }
    }

    pub async fn list_traces(&self, limit: u32, page: u32) -> Result<Vec<Trace>> {
        let url = format!("{}/api/public/traces", self.base_url);
        let resp = self
            .send_retrying(|| {
                self.http
                    .get(&url)
                    .query(&[("limit", limit.to_string()), ("page", page.to_string())])
            })
            .await?;
        let paged: PagedResponse<Trace> =
            resp.json().await.map_err(|e| Error::Parse(e.to_string()))?;
        Ok(paged.data)
    }

    pub async fn get_trace(&self, trace_id: &str) -> Result<Trace> {
        let mut url = reqwest::Url::parse(&format!("{}/api/public/traces", self.base_url))
            .map_err(|e| Error::Parse(e.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| Error::Parse("invalid Langfuse base URL".into()))?
            .push(trace_id);
        let resp = self.send_retrying(|| self.http.get(url.clone())).await?;
        resp.json().await.map_err(|e| Error::Parse(e.to_string()))
    }

    pub async fn get_observations(&self, trace_id: &str) -> Result<Vec<Observation>> {
        let url = format!("{}/api/public/observations", self.base_url);
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let page_str = page.to_string();
            let resp = self
                .send_retrying(|| {
                    self.http.get(&url).query(&[
                        ("traceId", trace_id),
                        ("limit", "100"),
                        ("page", page_str.as_str()),
                    ])
                })
                .await?;
            let paged: PagedResponse<Observation> =
                resp.json().await.map_err(|e| Error::Parse(e.to_string()))?;
            let count = paged.data.len();
            all.extend(paged.data);
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    pub async fn get_scores(&self, trace_id: &str) -> Result<Vec<Score>> {
        let url = format!("{}/api/public/scores", self.base_url);
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let page_str = page.to_string();
            let resp = self
                .send_retrying(|| {
                    self.http.get(&url).query(&[
                        ("traceId", trace_id),
                        ("limit", "100"),
                        ("page", page_str.as_str()),
                    ])
                })
                .await?;
            let paged: PagedResponse<Score> =
                resp.json().await.map_err(|e| Error::Parse(e.to_string()))?;
            let count = paged.data.len();
            all.extend(paged.data);
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    async fn send_retrying<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut attempt = 0u32;
        loop {
            match build().send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retryable = status == 429 || (500..=599).contains(&status);
                    if retryable && attempt < self.max_retries {
                        let delay = retry_after(&resp).unwrap_or_else(|| self.backoff(attempt));
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return map_status(resp).await;
                }
                Err(e) => {
                    if attempt < self.max_retries && (e.is_timeout() || e.is_connect()) {
                        tokio::time::sleep(self.backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        self.base_delay
            .checked_mul(factor)
            .unwrap_or(MAX_RETRY_DELAY)
            .min(MAX_RETRY_DELAY)
    }
}

fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let value = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    parse_retry_after(value)
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    let secs = value.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(secs).min(MAX_RETRY_DELAY))
}

async fn map_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status().as_u16();
    match status {
        200..=299 => Ok(resp),
        401 | 403 => Err(Error::Auth),
        404 => Err(Error::NotFound("resource not found".into())),
        429 => Err(Error::RateLimit),
        500..=599 => {
            let body = resp.text().await.unwrap_or_default();
            Err(Error::Upstream(format!("HTTP {status}: {body}")))
        }
        _ => {
            let body = resp.text().await.unwrap_or_default();
            let msg = if body.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {body}")
            };
            Err(Error::Parse(msg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn list_traces_sends_auth_and_parses_response() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{
                "id": "trace-1",
                "name": "my-trace",
                "timestamp": "2026-06-10T14:00:00.000Z",
                "latency": 1.5,
                "totalCost": 0.01,
                "tags": ["prod"]
            }],
            "meta": { "page": 1, "totalPages": 1, "totalItems": 1, "limit": 50 }
        });

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .and(query_param("limit", "50"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let traces = client.list_traces(50, 1).await.unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].id, "trace-1");
        assert_eq!(traces[0].name.as_deref(), Some("my-trace"));
    }

    #[tokio::test]
    async fn get_trace_returns_single_trace() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "id": "trace-abc",
            "name": "chat",
            "timestamp": "2026-06-10T14:00:00.000Z"
        });

        Mock::given(method("GET"))
            .and(path("/api/public/traces/trace-abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let trace = client.get_trace("trace-abc").await.unwrap();
        assert_eq!(trace.id, "trace-abc");
    }

    #[tokio::test]
    async fn get_observations_parses_list() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{
                "id": "obs-1",
                "traceId": "trace-1",
                "type": "GENERATION",
                "name": "llm-call",
                "startTime": "2026-06-10T14:00:00.000Z",
                "endTime": "2026-06-10T14:00:01.000Z",
                "model": "gpt-4o",
                "usage": { "input": 100, "output": 50, "total": 150 },
                "calculatedTotalCost": 0.002
            }]
        });

        Mock::given(method("GET"))
            .and(path("/api/public/observations"))
            .and(query_param("traceId", "trace-1"))
            .and(query_param("limit", "100"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let obs = client.get_observations("trace-1").await.unwrap();
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].kind, crate::model::SpanKind::Generation);
        assert_eq!(obs[0].model.as_deref(), Some("gpt-4o"));
    }

    #[tokio::test]
    async fn returns_auth_error_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let err = client.list_traces(50, 1).await.unwrap_err();
        assert!(matches!(err, Error::Auth));
    }

    #[tokio::test]
    async fn returns_rate_limit_on_429() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let err = client.list_traces(50, 1).await.unwrap_err();
        assert!(matches!(err, Error::RateLimit));
    }

    #[tokio::test]
    async fn returns_upstream_error_on_500() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let err = client.list_traces(50, 1).await.unwrap_err();
        assert!(matches!(err, Error::Upstream(_)));
    }

    #[tokio::test]
    async fn get_scores_parses_response() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": [
                {
                    "id": "s-1",
                    "traceId": "trace-1",
                    "name": "accuracy",
                    "value": 0.95,
                    "source": "API"
                },
                {
                    "id": "s-2",
                    "traceId": "trace-1",
                    "name": "sentiment",
                    "stringValue": "positive",
                    "dataType": "CATEGORICAL"
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/api/public/scores"))
            .and(query_param("traceId", "trace-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let scores = client.get_scores("trace-1").await.unwrap();
        assert_eq!(scores.len(), 2);
        assert_eq!(scores[0].name, "accuracy");
        assert_eq!(scores[0].value, Some(0.95));
        assert_eq!(scores[1].name, "sentiment");
        assert_eq!(scores[1].string_value.as_deref(), Some("positive"));
    }

    #[tokio::test]
    async fn list_traces_includes_auth_header() {
        let server = MockServer::start().await;
        let body = serde_json::json!({ "data": [] });

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .and(header("authorization", "Basic cGs6c2s="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::new(&server.uri(), "pk", "sk").unwrap();
        let traces = client.list_traces(50, 1).await.unwrap();
        assert!(traces.is_empty());
    }

    #[tokio::test]
    async fn get_observations_follows_pagination() {
        let server = MockServer::start().await;

        let page1: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "id": format!("obs-{i}"),
                    "traceId": "trace-1",
                    "type": "SPAN",
                    "name": "span",
                    "startTime": "2026-06-10T14:00:00.000Z",
                    "endTime": "2026-06-10T14:00:01.000Z"
                })
            })
            .collect();

        Mock::given(method("GET"))
            .and(path("/api/public/observations"))
            .and(query_param("page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": page1 })),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/observations"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "obs-last",
                    "traceId": "trace-1",
                    "type": "SPAN",
                    "name": "span",
                    "startTime": "2026-06-10T14:00:00.000Z",
                    "endTime": "2026-06-10T14:00:01.000Z"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let obs = client.get_observations("trace-1").await.unwrap();
        assert_eq!(obs.len(), 101);
        assert_eq!(obs[100].id, "obs-last");
    }

    #[tokio::test]
    async fn get_scores_follows_pagination() {
        let server = MockServer::start().await;

        let page1: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "id": format!("s-{i}"),
                    "traceId": "trace-1",
                    "name": "accuracy",
                    "value": 0.5
                })
            })
            .collect();

        Mock::given(method("GET"))
            .and(path("/api/public/scores"))
            .and(query_param("page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": page1 })),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/scores"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "s-last", "traceId": "trace-1", "name": "accuracy", "value": 0.9 }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let scores = client.get_scores("trace-1").await.unwrap();
        assert_eq!(scores.len(), 101);
        assert_eq!(scores[100].id, "s-last");
    }

    #[tokio::test]
    async fn retries_transient_5xx_then_succeeds() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let traces = client.list_traces(50, 1).await.unwrap();
        assert!(traces.is_empty());
    }

    #[tokio::test]
    async fn gives_up_after_exhausting_retries() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1 + MAX_RETRIES as u64)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let err = client.list_traces(50, 1).await.unwrap_err();
        assert!(matches!(err, Error::Upstream(_)));
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/public/traces"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": [] })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let traces = client.list_traces(50, 1).await.unwrap();
        assert!(traces.is_empty());
    }

    #[tokio::test]
    async fn get_trace_percent_encodes_id() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "id": "a/b c",
            "name": "chat",
            "timestamp": "2026-06-10T14:00:00.000Z"
        });

        Mock::given(method("GET"))
            .and(path("/api/public/traces/a%2Fb%20c"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::with_base_url(server.uri());
        let trace = client.get_trace("a/b c").await.unwrap();
        assert_eq!(trace.id, "a/b c");
    }

    #[test]
    fn parse_retry_after_reads_seconds() {
        assert_eq!(parse_retry_after("3"), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after("  5 "), Some(Duration::from_secs(5)));
    }

    #[test]
    fn parse_retry_after_rejects_non_numeric() {
        assert_eq!(parse_retry_after("soon"), None);
        assert_eq!(parse_retry_after(""), None);
    }

    #[test]
    fn parse_retry_after_caps_at_max() {
        assert_eq!(parse_retry_after("100"), Some(MAX_RETRY_DELAY));
    }
}
