# Next

State carried over from the old Mac on 2026-11-12.

## In flight
- Inactive. `main` is at bebcf4b, CI green, `Cargo.toml` at 0.3.0, no git tags, 99 tests passing locally.
- Seven Dependabot PRs are open (#1, #2, #3, #6, #7, #8, #9), all with green CI. #2 bumps tower-http 0.6 to 0.7, a breaking range for a 0.x crate; the rest are patch or action bumps.
- `CHANGELOG.md` stops at 0.3.0. The code commits after the release (114fede..fc11390: Host-header guard, Langfuse retries, bounded detail cache, `base_url` scheme check, auto-refresh toggle) are not listed.
