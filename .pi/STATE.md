# Current state

## Desktop app (macOS Apple Silicon and Linux x86_64)

Plan and status live in docs/desktop/ (README, phase docs, protocol). The owner chose a Tauri v2 app with a real Svelte 5 + TypeScript GUI, a Rooxcode-branded bundle ID, Linux x86_64 only at first, and unsigned builds (no Apple/Windows developer accounts). Windows is deferred.

Phase 0 is done: public hibiscus::desktop (Engine per workspace, save_api_key, sign_out, CodexSignIn, key_page, sessions, providers); Session::open takes the workspace explicitly.

Phase 1 is mostly implemented in desktop/ and on its way to dev as one PR: a Claude-style three-column window (frosted sidebar and overlay title bar on macOS), chat with sanitized Markdown, highlight.js code blocks and copy buttons, grouped tool rows with edit/write diffs, a session details panel, a branch diff pane (against origin/HEAD or main), image attachments (paperclip, paste, drop), in-app dropdowns, and Settings > Providers with API keys, Codex browser sign-in and sign-out. Codex models are discovered from the plan for every frontend (see MEMORY).

Verified: 236 unit tests, 110 native fixtures, tests/desktop_api.rs, strict Clippy and fmt; desktop glue 3 tests and Clippy; frontend svelte-check, 19 Vitest tests and a production build; live Codex model listing and one prompt to a discovered model. Not verified by hand: the window's look (screenshots came from the owner), light mode, Linux, and a full Codex browser sign-in from the app.

Next: release.yml desktop jobs into the same draft release (SHA256SUMS + provenance), install-desktop.sh with tests, a desktop CI job, a docs/installation.md section, confirming the bundle ID domain (placeholder com.rooxcode.hibiscus), and image tools (read images for vision models, an image-generation extension, inline display).

v0.3.30 (Codex plan model discovery for the TUI and CLI) shipped from main and is back in dev; its draft release holds all six files. Publish releases only after the tag-triggered workflow creates the draft.
