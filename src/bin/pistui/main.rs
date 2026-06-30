mod cli;
mod server;

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Args::parse();

    eprintln!(
        "connecting to {} (key: {}…)",
        args.host,
        &args.public_key[..args.public_key.len().min(8)]
    );

    let client = pistui::langfuse::Client::new(&args.host, &args.public_key, &args.secret_key)
        .context("failed to build Langfuse client")?;

    let cache = Arc::new(pistui::cache::TraceCache::new(client));

    match cache.warm().await {
        Ok(()) => eprintln!("loaded {} traces", cache.traces().await.len()),
        Err(e) => eprintln!("warning: initial fetch failed: {e}"),
    }

    let env_explain = build_env_explain();

    let state = std::sync::Arc::new(pistui::server::AppState {
        cache,
        env_explain,
        langfuse_host: Some(args.host.clone()),
        focus_trace: args.trace.clone(),
    });
    let app = server::router_with_state(state);

    let addr = format!("127.0.0.1:{}", args.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind to {addr}"))?;

    let local_addr = listener.local_addr()?;
    let url = format!("http://{local_addr}");
    eprintln!("pistui → {url}");

    if !args.no_open {
        let _ = open::that(&url);
    }

    axum::serve(listener, app).await?;
    Ok(())
}

fn build_env_explain() -> Option<pistui::explain::ExplainClient> {
    let api_key = std::env::var("LLM_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .ok();

    let base_url =
        std::env::var("LLM_BASE_URL").unwrap_or_else(|_| pistui::explain::DEFAULT_BASE_URL.into());

    let model =
        std::env::var("LLM_MODEL").unwrap_or_else(|_| pistui::explain::DEFAULT_MODEL.into());

    // ANTHROPIC_API_KEY needs a different base_url — the Anthropic API is not
    // OpenAI-compatible. Users should set LLM_BASE_URL alongside it.
    if api_key.is_none() {
        if let Ok(anthropic_key) = std::env::var("ANTHROPIC_API_KEY") {
            if std::env::var("LLM_BASE_URL").is_err() {
                eprintln!(
                    "warning: ANTHROPIC_API_KEY is set but LLM_BASE_URL is not — \
                     set LLM_BASE_URL to the Anthropic-compatible endpoint and LLM_MODEL \
                     to use it for explain"
                );
                let _ = anthropic_key;
                return None;
            }
        }
    }

    let api_key = api_key?;

    match pistui::explain::ExplainClient::new(&api_key, &base_url, &model) {
        Ok(client) => {
            eprintln!("explain enabled ({model} via {base_url})");
            Some(client)
        }
        Err(e) => {
            eprintln!("warning: explain setup failed: {e}");
            None
        }
    }
}
