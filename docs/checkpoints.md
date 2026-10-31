# Undo native file changes

Hibiscus checkpoints covered workspace files before its native `write` and `edit` tools change them. Ask Hibi in a normal conversation:

```text
Undo your changes from my previous request, but keep my existing work.
```

Hibi can inspect checkpoint history and the recorded diff, then restore all files from that request or a selected subset. Both CLI and TUI use the same tools. No new slash command or approval setting is required.

## What a checkpoint protects

A checkpoint starts lazily when a native file write is attempted. It groups the request's edits across model steps. Steering messages and queued followups each start a separate group. File-tool results include the checkpoint ID and whether the path is protected; the ID is also retained in the session's tool result metadata.

The first write captures the file as it exists at that moment, including your existing uncommitted edits. Repeated writes keep that original and update the recorded result. Ordinary permission bits, including executable permissions, are retained. Original bytes may be binary when `write` replaces a file; `edit` still requires UTF-8. A new file records an absent original.

Recovery data lives outside Git under `~/.hibiscus/checkpoints/<workspace-hash>/`. `HIBISCUS_HOME` overrides the data home. Files and metadata are private, with 0600 files and 0700 directories. The hash scopes records to the canonical workspace path. Moving a workspace changes that identity. Session compaction does not delete checkpoints.

Before a protected replacement, Hibiscus saves the original and planned result images plus a pending-write journal. A capture or storage error stops the protected edit. Native history reports pending operations instead of assuming they completed. No Git commands, commits, index updates, resets or stashes implement checkpoint recovery.

## Agent tools

| Tool | Purpose |
| --- | --- |
| `checkpoint_history` | List checkpoint IDs, sessions, active state, file counts and pending/restored counts |
| `checkpoint_diff` | Inspect recorded before/result changes and current conflicts |
| `checkpoint_restore` | Restore one request or selected files after preflight checks |
| `checkpoint_prune` | Explicitly remove old resolved history and unreferenced images |

History defaults to the current session and accepts `{"limit":10}`, with a maximum of 50. Use `{"allSessions":true}` to inspect other conversations in this workspace; the response includes `currentSession` for correlation. Diff and restore accept:

```json
{"checkpoint":"<ID from history>","paths":["src/example.rs"]}
```

Omit `paths` to select every file in the checkpoint. Paths must be unique normal relative UTF-8 paths belonging to that checkpoint. Diffs are bounded previews; request fewer paths when output is truncated. Both frontends can display the recorded diff, and `/output` can inspect the saved native result without rerunning it.

## Restore and conflicts

Restore first checks every selected file against its recorded resulting bytes and permissions, and verifies the retained original images. A conflict in any selected file stops the preflight without changing any of those files. You can choose an unaffected subset instead. It does not automatically merge later edits inside the same file.

An active user request holds a lease and cannot be restored until it finishes. For a completed request, restore puts overwritten files back exactly and removes newly created files only when they still match the recorded result. It preserves unrelated files and Git state. Empty parent directories created for new files remain. Ownership, timestamps, hard-link identity and directory metadata are not restored.

A restore journals its selected paths before changing them. Cancellation, an I/O error or process interruption can leave a partial restore. Inspect `checkpoint_diff` and retry the same checkpoint and paths. Files already restored are idempotent while they still match the original; unexpected content remains a conflict. Recovery never re-executes the original tool call. A missing/corrupt original image stops restoration before any selected file changes.

Native operations serialize through a nonblocking workspace lock. External editors and shell programs do not take that lock, so do not edit a selected file concurrently with its checked replacement. Observed later edits cause a conflict; this is not a filesystem-wide transaction against arbitrary processes.

## Coverage and limits

Initial coverage is native `write` and `edit` on regular workspace files. Shell commands and extension programs can modify files independently and are not automatically checkpointed. Native writes outside the workspace, inside Hibiscus's data home or to Git metadata explicitly report that they are unprotected. Symbolic-link and special-file targets are unsupported. Checkpoints are separate from extension version history.

Limits are 16 MiB per file, 32 files and 64 MiB of original/result image sizes per request, 256 correlated file calls, 512 bytes per relative path, and a 256 KiB record. A workspace retains at most 256 request records, 512 MiB of images and 8,192 image entries. Images share content hashes to avoid duplicate payloads, but per-request accounting counts each file's before/result size.

History is not removed automatically. At capacity, protected edits fail before replacement and direct the agent to the pruning tool. `checkpoint_prune` accepts `{"keep":20}`, with a range of 1–256. It keeps that many newest inactive resolved checkpoints, plus every active or pending operation. It then removes unreferenced image files. Pruning deletes recovery history, so retain the checkpoints you still need. Invalid records stop pruning conservatively. Backups of this private directory can contain project secrets and must not be published.
