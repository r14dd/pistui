# Changelog

## 0.3.0

**Rust / API**

- Orphan observations now re-rooted to tree top instead of dropped; `stats.orphanCount` exposes count
- TTFT (time-to-first-token) computed for Generation spans and included in tree nodes and LLM explain prompt
- `tokens_total` field on span nodes for providers that return an aggregate usage value
- `span.metadata` forwarded to API and surfaced in detail panel
- Trace-level `input`/`output` returned in detail response
- Trace scores support paginated fetching (fixed `traceId` casing in scores query parameter)
- 5xx responses from Langfuse mapped to `Error::Upstream` (BAD_GATEWAY), not INTERNAL_SERVER_ERROR
- `ExplainClient::new` returns `Result` instead of panicking on construction failure
- Explain prompt pruned at 60 nodes with `[N spans omitted]` note to stay within context limits
- Auth header omitted when API key is empty (Ollama / keyless local providers)
- `ANTHROPIC_API_KEY` without `LLM_BASE_URL` emits a warning instead of silently failing
- List endpoint returns lean DTO (no input/output blobs) for faster initial load
- Detail endpoint deduplicates with a shared `detail_body` builder
- `Error::Explain` and `Error::Upstream` both map to BAD_GATEWAY at the server layer

**UI**

- Trace-level input/output panel at top of detail view, before metadata
- Tags shown in metadata bar
- "Metadata" tab in span detail panel when `node.metadata` is set
- TTFT displayed on generation rows in span tree
- Per-span score badges inline on node rows with Boolean/categorical/numeric formatting
- Score values in trace-level scores section format by `dataType` (BOOLEAN, CATEGORICAL, numeric); span-level scores appear only as inline badges, not duplicated in this section
- Detail view shows a notice when a trace is trimmed (capped at 2000 observations) or has re-rooted orphan spans
- Compare button toggles a `comparing` flag so click flow works without pre-selecting rows
- j/k navigation tracks by trace ID so selection survives sort and filter changes
- Loaded-at staleness timestamp in filter bar
- Copy clipboard calls include `.catch` handlers
- Explain button relabels back to "explain" (not "explain this trace") after run completes
- Explain 502 no longer wipes saved API key; only 400 (bad key) does
- Copy span ID visible at low opacity by default (not invisible until hover); keyboard accessible
- Non-color error indicator on error span names (underline)
- CSS design tokens replace hardcoded hex in metadata bar, model breakdown, scores, help panel
- Animations gated behind `prefers-reduced-motion` check
- `word-break: break-word` instead of `break-all` in detail content pre blocks

## 0.2.0

- Scores: fetch and display Langfuse scores (numeric, categorical, with comments) on trace detail
- Per-model token/cost breakdown table in trace stats
- Trace metadata panel showing user ID, session ID, release, and timestamp
- JSON export: download full trace data (tree, stats, scores) as a file
- Keyboard shortcut help overlay (press `?`)
- Auto-refresh API for polling new traces on an interval
- Observations and scores fetched concurrently for faster trace detail loading

## 0.1.0

Initial release.

- Langfuse REST client with Basic auth, paginated fetching, and rate limit handling
- Trace parser: flat observations to span tree with stats (token counts, cost, error detection, nesting depth)
- In-memory trace cache with warm-on-startup and manual refresh
- Local web server (axum) with embedded static assets (single binary)
- Trace list view: sortable columns, text filter, keyboard navigation (j/k/Enter)
- Span tree view: expand/collapse, latency waterfall bars, input/output panels, error highlighting
- Filter spans by name, model, status, or kind
- Session view: group traces by session_id
- Compare mode: side-by-side trace comparison with stats diff
- Copy trace and span IDs to clipboard
- "Open in Langfuse" link on trace detail
- Optional LLM explain: pipe trace data through an OpenAI-compatible API for summaries
- Single-trace focus mode via positional CLI argument
- Loading skeletons and empty states with action hints
- Dark theme, mobile-responsive layout
