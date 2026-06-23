# Changelog

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
