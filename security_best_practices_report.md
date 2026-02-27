# Security Best Practices Report

## Executive Summary

A focused security review of the codebase (backend code inspection + dependency audit) found no remaining known exploitable vulnerabilities after remediation. Two medium-risk issues were identified and fixed in this change set. Current `cargo audit` output reports only non-vulnerability warnings (unmaintained/yanked packages), not CVEs.

Scan date: 2026-02-26  
Primary commands: `cargo check`, `cargo test -p sequoia-server -- --nocapture`, `cargo audit`

## Critical

No critical findings.

## High

No high-severity findings.

## Medium

### SBP-001: Transitive vulnerable crypto dependency in server dependency graph (Remediated)

- Severity: Medium
- Impact: A known `rsa` timing-sidechannel advisory (`RUSTSEC-2023-0071`) was pulled transitively through SQLx’s MySQL path, increasing supply-chain risk.
- Evidence:
  - Server dependency migration away from top-level `sqlx` crate: [server/Cargo.toml](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/Cargo.toml:23)
  - New Postgres-only SQLx compatibility shim: [server/src/db_sqlx.rs](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/src/db_sqlx.rs:1)
  - Crate-level alias/re-exports used by server modules: [server/src/main.rs](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/src/main.rs:9)
- Remediation:
  - Replaced top-level `sqlx` dependency with `sqlx-core` + `sqlx-postgres`.
  - Removed MySQL/SQLite transitive pull-in from the resolved lock graph.
  - Re-ran `cargo audit`; vulnerability is no longer reported.

### SBP-002: Guild name path handling allowed unsafe path/query characters (Remediated)

- Severity: Medium
- Impact: Unvalidated guild path values could include control/path/query delimiter characters, creating path-manipulation edge cases and avoidable upstream request abuse.
- Evidence:
  - Guild endpoint now normalizes and validates input before use: [server/src/routes/api.rs](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/src/routes/api.rs:204)
  - Validation rules and rejected characters: [server/src/routes/api.rs](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/src/routes/api.rs:263)
  - Safe URL path-segment construction via `reqwest::Url`: [server/src/routes/api.rs](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/src/routes/api.rs:279)
  - Added tests for invalid inputs and URL encoding: [server/src/routes/api.rs](/home/notes/Projects/sequoia-map-refactor/sequoia-map/server/src/routes/api.rs:412)
- Remediation:
  - Added strict guild-name normalization and rejection for dangerous characters.
  - Switched URL construction to encoded path segments.

## Low

No low-severity findings requiring immediate code changes.

## Informational / Residual Risk

### SBP-003: Dependency hygiene warnings (Open, non-CVE)

- `cargo audit` reports:
  - `RUSTSEC-2024-0436` (`paste`) as unmaintained (warning).
  - Yanked versions for `js-sys` and `wasm-bindgen` (warning).
- These are currently warnings (not active vulnerabilities), primarily in the client-side dependency tree.
- Recommendation:
  - Plan dependency upgrades for the frontend stack (`leptos`/`wgpu` transitive chain) to clear yanked/unmaintained packages.

## Verification Notes

- Backend compiles after remediation: `cargo check`
- Server tests pass: `cargo test -p sequoia-server -- --nocapture`
- Vulnerability scan passes with warnings only: `cargo audit`

