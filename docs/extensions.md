# Local extensions

The executable extension API is available from v0.3.6. You can also build this checkout with `cargo run --locked --` or `cargo install --path . --locked`.

Extensions add model-callable tools and local slash commands to both CLI and TUI. They are ordinary programs using a versioned JSON protocol. Hibiscus does not require an extension SDK, Pi or Node. Each extension needs its chosen executable or interpreter installed.

## Create an extension

The v0.3.6 installer and application startup create an empty `~/.hibiscus/extensions/` folder. `HIBISCUS_HOME` overrides the data directory. Existing extensions and settings are preserved.

Ask Hibi in an ordinary conversation:

```text
Create an extension that reviews this project's docs and adds /review.
```

The agent has three native tools:

| Tool | Purpose |
| --- | --- |
| `create_extension` | Write a complete manifest and source files, then register the extension |
| `list_extensions` | Inspect loaded extensions, directories, tools, commands and errors |
| `reload_extensions` | Reload manifests after editing an existing extension |

`create_extension` accepts `{"manifest": {...}, "files": [{"path": "main.py", "content": "...", "executable": false}]}`. The manifest follows the format below. The agent chooses the implementation language and source; Hibiscus does not generate a fixed template. Files are relative to the new extension directory. Creation validates the manifest, names, file paths and catalog limits before writing. Existing extension directories are never overwritten. Source is limited to 32 files, 64 KiB per file and 1 MiB total; directories and executable files have mode 0700, ordinary files and the manifest 0600. Set `executable: true` for a program invoked directly.

A created extension might look like:

```text
~/.hibiscus/extensions/project_review/
├── extension.json
└── main.py
```

Successful creation registers its tools for the next provider step and its slash commands for the next user input in the same session. To change an existing extension, ask the agent to edit its files and call `reload_extensions`. You can also maintain the files yourself; startup discovers installed manifests.

Discovery reads only global extension manifests and does not execute their programs. Workspace extensions are not discovered automatically. To opt into code maintained elsewhere, place or link its extension directory under your own Hibiscus extensions directory. Directory names must match manifest IDs.

A malformed extension is disabled as a whole, with a reported error; other extensions and native tools remain usable. A failure to read the entire directory during reload returns an error and retains the previous catalog. Each completed provider response executes its tool batch against one registry snapshot. Agent creation/reload publishes a new snapshot for the following provider request. Program files are read when invoked; reload after changing their manifest contract.

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

IDs and local tool/command names start with a lowercase ASCII letter and contain at most 28 lowercase letters, digits or underscores. Descriptions are nonempty, at most 1,000 bytes, with no control characters. A manifest is at most 64 KiB and may register up to 32 tools and 32 commands. The loader scans up to 128 directory entries and considers up to 32 extension directories, with catalog limits of 128 combined native/extension tools (currently 120 extension tools) and 128 extension commands. Keep manifests and catalogs small so tool descriptions fit your model's context.

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
  "extensionDir": "/home/user/.hibiscus/extensions/hello",
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
