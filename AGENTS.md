# Hibiscus agent instructions

These instructions apply when working in this repository.

## Project rules

- Hibiscus is a standalone Rust coding agent. The owner approved replacing the Pi SDK and runtime on 2026-10-29. Hibiscus owns providers, tools, authentication, sessions and the agent loop. Never restore a Pi/Node dependency or silently read Pi credentials.
- The local UI adapter uses bounded JSONL over an in-process Unix socket. Keep typed provider/session/tool data behind that adapter. Correlate responses by ID; prompt acceptance is not completion. Wait for run settlement after all tool calls and queued messages finish. Never execute tool calls from an incomplete provider stream.
- Make targeted changes and add regression tests for bugs when feasible. Never claim tests passed without running them; record missing tooling and other verification blockers explicitly.
- Do not put credentials, private session transcripts, or user-specific paths in committed records.
