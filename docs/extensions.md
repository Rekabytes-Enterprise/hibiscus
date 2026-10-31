# Local extensions

The executable extension API is available from v0.3.6. You can also build this checkout with `cargo run --locked --` or `cargo install --path . --locked`.

Extensions add model-callable tools and local slash commands to both CLI and TUI. They are ordinary programs using a versioned JSON protocol. Hibiscus does not require an extension SDK, Pi or Node. Each extension needs its chosen executable or interpreter installed.

## Create an extension

The v0.3.6 installer and application startup create an empty `~/.hibiscus/extensions/` folder. `HIBISCUS_HOME` overrides the data directory. Existing extensions and settings are preserved.

Ask Hibi in an ordinary conversation:

```text
Create an extension that reviews this project's docs and adds /review.
```

The agent has these native tools:

| Tool | Purpose |
| --- | --- |
| `create_extension` | Write a complete manifest and source files, then register the extension |
| `list_extensions` | Inspect loaded extensions, directories, tools, commands and errors |
| `reload_extensions` | Import changed editable source while keeping a valid active version on failure |
| `update_extension` | Prepare, validate and activate a candidate update |
| `extension_history` | Inspect versions, parents, changed files and validation results |
| `extension_diff` | Compare two retained versions or candidates without executing code |
| `rollback_extension` | Revalidate and restore a retained version as a new activation |

`create_extension` accepts `{"manifest": {...}, "files": [{"path": "main.py", "content": "...", "executable": false}]}`. The manifest follows the format below. The agent chooses the implementation language and source; Hibiscus does not generate a fixed template. Files are relative to the new extension directory. Creation validates the manifest, names, file paths and catalog limits before writing. Existing extension directories are never overwritten. Source is limited to 32 files, 64 KiB per file and 1 MiB total; directories and executable files have mode 0700, ordinary files and the manifest 0600. Set `executable: true` for a program invoked directly.

A created extension might look like:

```text
~/.hibiscus/extensions/project_review/
├── extension.json
└── main.py
```

Successful creation registers its tools for the next provider step and its slash commands for the next user input in the same session. To change an existing extension, ask the agent to use `update_extension`. The agent reads its current version from `list_extensions`, supplies that `expectedVersion`, patches source files and optionally supplies a replacement manifest. Unspecified files remain intact. See the version workflow below. You can still edit the original source folder and explicitly call `reload_extensions`; startup keeps the last active version after the first import.

Discovery reads only global extension manifests and does not execute their programs. Workspace extensions are not discovered automatically. To opt into code maintained elsewhere, place or link its extension directory under your own Hibiscus extensions directory. Directory names must match manifest IDs.

A malformed extension with no active version is disabled as a whole, with a reported error; other extensions and native tools remain usable. A rejected update to an installed extension keeps its previous version active. A failure to read the entire directory during reload returns an error and retains the previous catalog. Each completed provider response executes its tool batch against one registry snapshot. Agent creation/reload publishes a new snapshot for the following provider request. Each registry resolves both metadata and program paths from its own retained version bundle. An update never replaces the files used by an older registry or a running invocation.

## Updates, checks and rollback

Managed history lives beside the editable source, under a private directory:

```text
~/.hibiscus/extensions/
├── hello/                       # Original editable source, preserved
└── .versions/hello/
    ├── active.json              # Atomic active-version pointer
    ├── candidates/<version>/
    │   ├── record.json
    │   └── files/
    └── versions/<version>/
        ├── record.json
        └── files/
```

First discovery snapshots an existing extension without executing it. The record marks that import separately from an update that passed executable regression checks. Directory symlinks remain an explicit opt-in; files within the directory must be regular files, with normal relative UTF-8 paths. Nested symlinks and special files are rejected. A snapshot permits 256 files, 512 directory entries, 16 MiB per file and 32 MiB total. Snapshot directories are 0700 and files 0600 or 0700 for executables.

A typical agent update is:

```json
{
  "id": "hello",
  "expectedVersion": "<active version from list_extensions>",
  "files": [{"path": "main.py", "content": "...complete replacement file..."}],
  "remove": ["obsolete.txt"]
}
```

`files` is a patch over the active snapshot. It preserves all unspecified files. `remove` explicitly deletes regular files; remove empty directories yourself in the editable source when restructuring it. `manifest` optionally replaces the entire manifest and must retain its ID. `extension.json` is reserved in both patch lists. Each patch allows 32 writes, 32 removals, 64 KiB per written file and 1 MiB total. Use `fromSource: true` to import your edited installation folder instead of starting from the active version.

Before activation, Hibiscus validates the whole manifest, JSON schemas, names and catalog collisions, file paths, executable availability and implementation files referenced with `{extensionDir}`. It then runs the manifest's optional `checks` against a separate copy. Checks may produce caches and reports without changing the candidate payload:

```json
"checks": [
  {"kind": "exec", "command": ["python3", "-m", "unittest", "-v"], "timeoutSeconds": 30},
  {"kind": "tool", "name": "greet", "arguments": {"name": "test"}},
  {"kind": "command", "name": "greet", "input": "test"}
]
```

There may be up to eight checks. `exec` runs a direct argument array from the validation copy, with a default 30-second timeout and a permitted range of 1–600 seconds. It must exit zero. Tool and command checks use local names and the normal JSON protocol, argument validation, manifest timeout and logical-error handling. Each output stream is bounded to 64 KiB; cancellation and failure terminate the process group. Tool/command probes run in the current workspace and can perform the extension's real side effects. Only configure checks that are appropriate to run there.

Discovery never runs checks. First-time imports record their deferred check count. An explicit reload imports edited source with structural validation when it has no checks. If checks are configured, reload reports that a checked import is required; ask the agent to call `update_extension` with `fromSource: true`. Later updates and rollback automatically run the selected manifest's checks. Omitting `manifest` preserves those checks.

The complete bundle and its record reach retained storage before Hibiscus atomically publishes the active pointer. The pointer names both the implementation and its metadata. `expectedVersion` and a commit lock reject concurrent stale updates. Registrations appear on the next model step and user command, while an already-running tool batch keeps its original bundle. Other Hibiscus processes adopt changes on restart or explicit reload.

`extension_history` takes `{"id":"hello","limit":20}`. It returns active version, parents, restored versions, changed paths, content hashes, validation status/results and candidate directories. The limit is 1–100 and the scan is bounded; `truncated` indicates omitted entries. `extension_diff` takes `{"id":"hello","from":"<version>","to":"<version or candidate>"}`. It reports added/changed/deleted paths, executable mode changes and a bounded text preview. Binary and large files receive a change notice.

`rollback_extension` takes `{"id":"hello","expectedVersion":"<current>","version":"<retained version>"}`. It revalidates and copies the retained version into a new candidate, then runs its checks before activation. Its parent is the current version and `restoredFrom` identifies the selected historical version. Rollback keeps every earlier version and the original author files.

On a validation failure, the candidate remains inspectable and the prior active version remains usable. A crash before pointer publication leaves the previous version active and a candidate or unreferenced bundle in history. A crash afterward leaves the complete new version active. Interrupted candidates show `preparing`; they are never resumed or executed automatically. Inspect their diff, then submit a fresh update against the current version. To recover a bad successful update, request rollback through the agent. Retained bundle integrity is checked on load and restore; modified bundles are reported rather than silently used. No automatic cleanup removes history. Back up this directory before manually removing old candidates or versions, and never remove an active or running version.

Keep implementation files inside the extension directory and refer to them with `{extensionDir}`. Absolute manifest command paths into the original source are rebased when imported. Store mutable extension state in the workspace or a separate user data directory. Treat `.versions` as managed, immutable storage. Installed interpreters, external programs and code paths embedded in your own implementation remain outside the snapshot. Version history does not sandbox trusted programs or make external dependencies reproducible.

## Manifest

```json
{
  "apiVersion": 1,
  "id": "hello",
  "description": "Example greeting extension",
  "command": ["python3", "{extensionDir}/main.py"],
  "timeoutSeconds": 15,
  "tools": [
    {
      "name": "greet",
      "description": "Greet someone by name",
      "parameters": {
        "type": "object",
        "properties": {"name": {"type": "string", "minLength": 1}},
        "required": ["name"],
        "additionalProperties": false
      }
    }
  ],
  "commands": [{"name": "greet", "description": "Print a local greeting", "slashCommand": "/hello"}]
}
```

The model sees tool `hello__greet`. Users run `/hello [input]`. `slashCommand` is optional: without it the command is `/hello:greet`. A chosen slash name starts with `/` and a lowercase letter, followed by lowercase letters, digits, underscores or hyphens, at most 64 characters after `/`. Extensions cannot replace native tools or commands, or reuse another extension's slash command. A collision rejects the affected extension's complete registration.

IDs and local tool/command names start with a lowercase ASCII letter and contain at most 28 lowercase letters, digits or underscores. Descriptions are nonempty, at most 1,000 bytes, with no control characters. A manifest is at most 64 KiB and may register up to 32 tools and 32 commands. The loader scans up to 128 directory entries and considers up to 32 extension directories, with catalog limits of 128 combined native/extension tools (currently 116 extension tools) and 128 extension commands. Keep manifests and catalogs small so tool descriptions fit your model's context.

`command` is an argument array of 1–32 nonempty strings, each at most 4,096 bytes. It runs directly without shell interpolation. `{extensionDir}` expands to the extension's absolute directory, including in arguments. A program path containing a slash is resolved relative to the extension directory unless already absolute; an unqualified program uses `PATH`. Relative ordinary arguments resolve from the workspace, so use `{extensionDir}` for script paths.

Tools require an object parameters schema. Hibiscus compiles Draft 7 JSON Schemas once and validates arguments before starting the program. Schemas must be self-contained; network and filesystem reference resolution are disabled. Patterns use Rust regex syntax. The [Rust JSON Schema validator](https://docs.rs/jsonschema/0.60.2/jsonschema/) supplies validation. Invalid schemas produce a load error. Tool arguments and command input are limited to 64 KiB.

`timeoutSeconds` defaults to 120 and accepts 1–600. A manifest must have at least one tool or command. Unknown manifest fields are errors, which helps catch misspellings.

## Request and reply

Hibiscus starts one process per invocation, in the current workspace. It writes one JSON request followed by a newline to stdin, then closes stdin. For a tool:

```json
{
  "apiVersion": 1,
  "id": "tool-0",
  "kind": "tool",
  "name": "greet",
  "arguments": {"name": "Raspberry"},
  "cwd": "/workspace/project",
  "extensionDir": "/home/user/.hibiscus/extensions/.versions/hello/versions/<version>/files",
  "sessionId": "current-session-id"
}
```

For a slash command, `kind` is `command`, `name` is the local command name and `arguments` is `{"input":"the remaining command text"}`. Input is passed as text, including shell punctuation; Hibiscus does not evaluate it as shell code. The environment also contains `HIBISCUS_EXTENSION_DIR` and `HIBISCUS_WORKSPACE`.

Keep stdin/stdout for the protocol and avoid reading directly from the terminal while Hibiscus owns input. Write one JSON object to stdout and exit:

```json
{
  "apiVersion": 1,
  "id": "tool-0",
  "text": "Hello, Raspberry!",
  "isError": false,
  "data": {"optional": "tool metadata"}
}
```

Echo the exact request `id`. `apiVersion`, `id` and `text` are required. `isError` defaults to false and `data` defaults to null. Unknown reply fields, mismatched IDs, invalid JSON or nonzero exits are errors. Put diagnostics on stderr; nonzero-exit errors include its bounded text. Each output stream is limited to 64 KiB. A request, including context, is limited to 128 KiB. Text is displayed without terminal control characters.

Model tool results keep their call IDs, error flags, actual execution duration and OS exit status in the existing tool/session flow. An extension can report a logical failure with `isError: true` even when its process exits zero. Optional `data` is retained under the tool result's extension metadata. `/output` can inspect the saved tool text.

Slash command output is display-only and stays outside model/session messages. Calling a custom slash command creates no provider request; the extension program controls its own external work. Asking Hibi to author or inspect extensions is a normal agent conversation, with native tool results saved in its history. Esc cancels a running extension; timeout, cancellation and output overflow terminate its process group. An extension error leaves interactive Hibiscus usable. Piped/one-shot failures remain nonzero, matching native command behavior.

## Authoring in another language

Change the manifest's `command` and implement the same stdin/stdout protocol. JavaScript can use Node when its author chooses that dependency:

```js
let input = "";
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
process.stdout.write(JSON.stringify({
  apiVersion: 1,
  id: request.id,
  text: "Hello from JavaScript!"
}) + "\n");
```

For that file, use `"command": ["node", "{extensionDir}/main.mjs"]`. Compiled executables can use `"command": ["{extensionDir}/my-program"]`. A complete optional Python example is in [examples/extensions/hello](../examples/extensions/hello/).

Extensions execute with the user's filesystem, network and inherited environment access. They are trusted local code and are not sandboxed. This API preserves direct execution without approval prompts.

Version 1 exposes tools and local commands. It does not expose provider replacement, lifecycle hooks, arbitrary frontend widgets or a package download/update manager. No persistent extension process runs between invocations.
