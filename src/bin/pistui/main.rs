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

    let app = server::router(cache, env_explain);

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
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .ok()?;

    let base_url =
        std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());

    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());

    eprintln!("explain enabled ({model} via {base_url})");
    Some(pistui::explain::ExplainClient::new(
        &api_key, &base_url, &model,
    ))
}
