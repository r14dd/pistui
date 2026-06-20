# pistui

A local web UI for exploring [Langfuse](https://langfuse.com) traces. Single binary, runs on localhost, opens in your browser.

![MIT/Apache-2.0](https://img.shields.io/crates/l/pistui)
![crates.io](https://img.shields.io/crates/v/pistui)

## What it does

pistui connects to your Langfuse instance (cloud or self-hosted), pulls down traces, and serves a local web interface for browsing them. Span trees, latency waterfall bars, token counts, cost breakdowns, keyboard navigation. Everything runs in memory after the initial fetch.

## Install

```
cargo install pistui
```

## Usage

```
pistui --host https://cloud.langfuse.com \
       --public-key pk-lf-... \
       --secret-key sk-lf-...
```

This starts a local server and opens your browser. You can also pass credentials as environment variables:

```
export LANGFUSE_HOST=https://cloud.langfuse.com
export LANGFUSE_PUBLIC_KEY=pk-lf-...
export LANGFUSE_SECRET_KEY=sk-lf-...
pistui
```

To jump straight to a specific trace:

```
pistui --host https://cloud.langfuse.com --public-key pk-... --secret-key sk-... TRACE_ID
```

### Options

| Flag | Env var | Description |
|------|---------|-------------|
| `--host` | `LANGFUSE_HOST` | Langfuse API URL |
| `--public-key` | `LANGFUSE_PUBLIC_KEY` | Langfuse public key |
| `--secret-key` | `LANGFUSE_SECRET_KEY` | Langfuse secret key |
| `--port` | | Local port (default: auto) |
| `--no-open` | | Don't open the browser |

## Features

**Trace list** with sortable columns (name, time, duration, cost, tags) and text filtering. Keyboard driven: `j`/`k` to move, `Enter` to open, `/` to filter, `R` to refresh.

**Span tree** with expand/collapse, latency waterfall bars, input/output panels, and error highlighting. Filter spans by name, model, status, or kind.

**Sessions** group traces by `session_id`. Toggle between trace list and session view.

**Compare mode** for viewing two traces side by side with a stats diff bar.

**Copy buttons** for trace and span IDs. "Open in Langfuse" link on trace details.

**Explain** (optional) pipes trace data through an LLM for a plain-language summary. Requires an OpenAI-compatible API key, either via env vars (`LLM_API_KEY`, `LLM_BASE_URL`, `LLM_MODEL`) or configured in the browser.

## Keyboard shortcuts

### Trace list

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `Enter` | Open selected trace |
| `/` | Focus filter input |
| `R` | Refresh traces |
| `Esc` | Clear filter focus |

### Span tree

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `Enter` | Expand / collapse node |
| `o` | Toggle input/output panel |
| `Esc` | Back to trace list |

## Architecture

Library crate with zero UI dependencies. All business logic (Langfuse client, trace parser, cache) is testable without a browser.

```
src/
  lib.rs, model.rs      types and error definitions
  langfuse.rs            REST client (Basic auth, paginated fetching)
  parser.rs              flat observations -> span tree (pure functions)
  cache.rs               in-memory trace cache
  explain.rs             optional LLM explain (OpenAI-compatible)
  server.rs              axum router, API routes, embedded static assets
  bin/pistui/            CLI binary

static/                  vanilla JS, no build step, embedded via rust-embed
  index.html, style.css, app.js, tree.js
```

## Development

```bash
# run with debug-embed (reads static/ from disk, no recompile for frontend changes)
cargo run -- --host https://cloud.langfuse.com --public-key pk-... --secret-key sk-...

# tests
cargo test                    # all tests
cargo test --lib              # unit tests only
cargo test --test integration # integration tests
cargo test --test e2e         # end-to-end tests

# lint
cargo clippy --all-targets
cargo fmt --check
```

The `debug-embed` feature means you can edit HTML/CSS/JS files and just refresh the browser. No Rust recompile needed for frontend work.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT), at your option.
