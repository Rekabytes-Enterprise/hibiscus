# Workspace checkpoint design

## Grounding

The engine executes completed provider tool batches and persists their correlated results. Each outer user prompt has a loop of provider steps; steering inserts a new user message at a tool boundary and follow-ups start a new outer iteration. Native write/edit currently share one atomic file writer, but retain no complete before-image. Saved edit previews are bounded presentation data, not recovery storage. Extension bundles own a separate lifecycle and must not become workspace backups.

## Candidate designs

A workspace-wide snapshot around each engine turn would capture shell changes too, but requires unbounded filesystem enumeration, exclusion rules and coordination with arbitrary processes. Git-backed snapshots would also couple recovery to the user's index and repository state. Reject this approach for the native write/edit scope.

A journal owned by the native file writer can persist exact before/result images before touching the target. The engine supplies request/session/call identity; the journal owns capture, checked writes, inspection, selective restore and retention. Choose this design. A single writer prevents a nearby new file tool from bypassing capture accidentally. No user checkpoint gate is required.

## Caller and ownership

```text
request = checkpoints::Request::new(session_id)
context = checkpoints::Context { cwd, request: &mut request, call_id }
tools::execute(name, arguments, context, steps, cancellation).await
    write/edit -> checkpoints::write(context, target, bytes, expected_read?)
    checkpoint tool -> checkpoints::Tool::execute(arguments, context, cancellation)
```

`src/checkpoints` owns typed workspace paths, file states, content-addressed images, journal records, leases, writes, restore and pruning. The engine starts a fresh request on ordinary, queued and steering user messages. Request leases live through its provider steps. Request IDs in receipts connect journal records with ordinary session tool results without changing the existing session wire format. CLI and TUI consume ordinary native tool results.

Store private records under `HIBISCUS_HOME/checkpoints/<workspace-hash>/requests/<request-id>` and immutable images under that workspace's `blobs`. The canonical workspace identity is checked when records load. Normal relative paths and validated content hashes cannot escape these directories. Symbolic links/special files are not checkpoint targets. Explicit outside-workspace, Git metadata and Hibiscus-data writes report that they are unprotected.

Before a checked write, save both original and planned result bytes, then persist a pending-write record. After the atomic target replacement, mark it applied. Repeated writes keep the first before-image and verify the current file still matches the preceding result. A crash can leave the previous or planned image; inspection reports pending work, and explicit restore reconciles those exact states. Unknown content is a conflict.

Restore first checks every selected file and verifies the retained images. It journals the selected restore before mutation, then marks each restored path after writing. Interruption can leave a partial restore; repeating the request reconciles before/result images and resumes without replaying tools. Already restored paths are idempotent only while they still match the recorded original. Later edits conflict. Native operations use a nonblocking workspace lock; per-request leases protect active groups from pruning. Non-cooperating external processes must not edit a target during the short checked-replacement operation.

## Bounds and retention

Bound files, records, paths, call traces, per-request image bytes and per-workspace storage. At capacity, fail before a protected mutation and report the pruning tool. Explicit pruning keeps the newest requested number of inactive resolved checkpoints, skips active or unfinished operations, and garbage-collects only unreferenced images. No implicit Git commands or automatic history deletion are needed.

## Verification plan

Exercise ordinary/native and synthetic-provider paths, existing uncommitted work, repeated edits, new files, permissions/binary originals, conflicts, symlink/path exclusions, corruption, failed capture, interrupted write/restore, non-Git projects, isolation, concurrent leases and pruning. Actual CLI/TUI conversations must edit files, inspect history/diff and restore, with authoritative file bytes checked independently of the model's prose. Post-merge captures use only isolated homes and synthetic providers.
