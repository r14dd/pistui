use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::time::Duration;

use crate::model::{Error, Observation, Result, Trace};

pub struct Client {
    http: reqwest::Client,
    base_url: String,
}

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
        Ok(Self { http, base_url })
    }

    #[doc(hidden)]
    pub fn with_base_url(base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
        }
    }

    pub async fn list_traces(&self, limit: u32, page: u32) -> Result<Vec<Trace>> {
        let url = format!(
            "{}/api/public/traces?limit={limit}&page={page}",
            self.base_url
        );
        let resp = self.http.get(&url).send().await?;
        let resp = map_status(resp).await?;
        let paged: PagedResponse<Trace> =
            resp.json().await.map_err(|e| Error::Parse(e.to_string()))?;
        Ok(paged.data)
    }

    pub async fn get_trace(&self, trace_id: &str) -> Result<Trace> {
        let url = format!("{}/api/public/traces/{trace_id}", self.base_url);
        let resp = self.http.get(&url).send().await?;
        let resp = map_status(resp).await?;
        resp.json().await.map_err(|e| Error::Parse(e.to_string()))
    }

    pub async fn get_observations(&self, trace_id: &str) -> Result<Vec<Observation>> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let url = format!(
                "{}/api/public/observations?traceId={trace_id}&limit=100&page={page}",
                self.base_url
            );
            let resp = self.http.get(&url).send().await?;
            let resp = map_status(resp).await?;
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
}

async fn map_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status().as_u16();
    match status {
        200..=299 => Ok(resp),
        401 | 403 => {
            let body = resp.text().await.unwrap_or_default();
            eprintln!("langfuse auth failed (HTTP {status}): {body}");
            Err(Error::Auth)
        }
        404 => Err(Error::NotFound("resource not found".into())),
        429 => Err(Error::RateLimit),
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
}
