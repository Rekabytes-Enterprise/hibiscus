# Extension versions

## Grounding and candidates

The engine holds one registry through each provider response's tool batch. The registry owns parsed manifests and compiled schemas, but its processes currently read mutable installation files. Keeping the registry alive therefore does not keep its implementation alive.

Candidate A backs up the installation directory and swaps it during updates. Callers stay simple, but a process already started can read replacement files through its old directory name. Publishing a manifest and files separately also leaves a crash window. Reject this design.

Candidate B stores complete version bundles separately and publishes one active-version pointer. Registries resolve both manifest and executable paths from that bundle. Old registries retain their own bundle, even after update or rollback. Choose B. The storage module owns snapshots, integrity, history, candidates and the atomic pointer; the engine only awaits the native tool dispatcher.

## Caller and module sketch

```text
Catalog.execute(Update, arguments, context, cancellation).await
    parse Update { id, expectedVersion, manifest?, files, remove, checks? }
    Store.prepare -> Candidate copied from the active bundle
    Candidate.validate -> full manifest + namespace + paths + executable checks
    Candidate.checks(context, cancellation).await -> recorded results
    Store.activate(candidate, expectedVersion) -> VersionRecord
    publish a registry that resolves immutable bundle directories

Catalog.execute(History | Diff | Rollback, ...).await
```

- `versions.rs` owns typed state/records, bounded private file snapshots, digests, candidate preparation, atomic activation and rollback. A nonblocking advisory lock serializes commits across processes. Rechecking the expected parent under this lock rejects stale updates. Candidate and version directories are never deleted automatically.
- `validation.rs` runs optional bounded regression commands and protocol probes against an expendable candidate copy. Discovery never runs code. Configured checks belong to the manifest so later updates and rollback cannot silently omit them.
- `agent.rs` parses native tool arguments and publishes catalog snapshots. No UI owns version storage or activation.
- `mod.rs` resolves active versions for registrations. Existing installations migrate on first discovery without replacing the author's source directory. Explicit reload imports changed source through a new validated candidate; failed imports retain the previous active version.

The active pointer is the commit point. A candidate and its complete record reach the versions directory first; a crash before pointer publication leaves the old version active and an inspectable candidate or orphan version. A crash afterward leaves the complete new version active. History records parent, content digest, changed paths and validation evidence. Rollback creates a new activation record derived from a retained version, so history remains append-only.

## Compatibility and limits

`extensions/<id>` remains the editable legacy source. Native updates change the managed active version and preserve those original files. Reload imports that source only when it differs from the last imported source digest. Use `update_extension` for subsequent managed updates. Root directory symlinks remain opt-in; nested symlinks and special files are rejected to prevent a snapshot escaping its directory. Installed interpreters and other external dependencies are outside the snapshot. Authors must keep extension implementation files inside the extension directory and use `{extensionDir}` for their paths.

Validation commands use the same cancellation, timeout and output limits as extension processes. They are trusted executable checks, not a sandbox. No approval system, extension-management slash command, download manager or provider runtime dependency is added.
