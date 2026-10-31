# Executable extension boundary

The extension API adds user-owned tools and local commands to the existing native runtime. It preserves one runtime/session, authoritative provider completion before execution, direct tool execution, call/result IDs, cancellation and UI independence. Author usage, manifests and JSON request/reply examples are in [extension authoring](../extensions.md).

## Shape

- `extensions/manifest.rs` parses bounded typed manifests, validates public slash names and compiles self-contained Draft 7 schemas with resolver features disabled and Rust regex patterns.
- `extensions/mod.rs` owns discovery, complete registration, native/extension collision checks, metadata and lookup. Invalid entries produce a report without executing code.
- `extensions/agent.rs` owns the `create_extension`, `list_extensions` and `reload_extensions` native agent tools. Creation takes a complete manifest plus arbitrary source files, checks current on-disk manifests and validates before writing, uses exclusive private directory/file creation, and writes the manifest last. Existing files are never overwritten. The installer and startup create an empty extensions directory.
- `extensions/agent.rs::Catalog` shares an `Arc<Registry>` snapshot behind a short-lived lock. Creation/reload publishes a replacement snapshot; the next provider step and user command see its registrations. A failed directory-wide reload preserves the prior snapshot; malformed individual extensions are disabled and reported in the replacement catalog.
- `extensions/process.rs` serializes one typed versioned request and parses one typed reply. It verifies the echoed correlation ID and version, then converts results into native tool/display data.
- `process.rs` owns concurrent bounded stdin/stdout/stderr, timeouts, cancellation, process-group termination and child reaping. Native shell tools and extensions use this same runner.
- `runtime/engine.rs` snapshots the catalog before each provider request and holds it through that response's tool batch. It advertises merged definitions through each provider adapter. Agent creation/reload and extension tools use the same completed-response execution/result path as other native tools. A newly created tool can run on the next model step of the same conversation.
- `commands.rs` is the authoritative native command catalog. Extension public names default to `/id:name` or choose `slashCommand`, with collisions checked against this shared catalog.
- Custom slash commands run as correlated cancellable runtime operations without constructing a model prompt or session message. Runtime metadata exposes command labels to the common frontend; TUI uses one filtered catalog for display, Tab and Enter. CLI accepts the same complete command names. Unknown slash tokens fail locally; absolute file paths containing another slash remain prompts.

Tool-call IDs and extension identities occupy separate fields: `create_extension` receives `arguments.manifest.id`, while the subprocess protocol echoes the invocation's correlation ID. Discovery and reload never execute entrypoints. Agent authoring/inspection tool results are saved normally in session/model history; invoking a custom slash command remains local and display-only.

## Synthesis decision

An in-process JavaScript or native shared-library API would expose application internals and introduce a language runtime or ABI compatibility contract. Executable extensions provide language choice through a smaller stable protocol and keep process failures separate from the agent. The selected API uses a fresh process per invocation; persistent plugins, hooks and arbitrary frontend widgets remain future API work.

For authoring, generic filesystem tools with restart-only discovery would leave the agent without immediate registration feedback. Dedicated create/list/reload agent tools instead own path validation, private creation and catalog publication. They require no built-in extension-management slash command or fixed starter language. This follows the owner's corrected workflow: an empty extensions folder at installation/startup, then agent-authored capabilities and custom slash names.

We retain the native adapter's JSON tool-definition/result boundary rather than rewriting all provider/session types in the same feature. Incoming manifests and replies are typed and bounded at their boundaries. Registry handles hide process and schema details from the engine. Tool namespaces and checked public slash names preserve native registrations.

## Verification

Unit tests execute real local programs and validate whole-manifest rejection, command collisions, source paths/private permissions, no-overwrite creation, schemas before side effects, invalid versions/references/replies, output overflow and timeout. Integration tests run the actual binary against synthetic providers for all three APIs and persist tool results. They reject execution after incomplete streams, create an extension and invoke its tool on the next provider step, inspect errors through agent tools, and exercise custom slash commands and TUI completion after creation/reload. CLI/TUI PTYs verify Esc kills descendants and permits a subsequent prompt. Custom slash invocations make no provider requests and remain outside model/session history. Physical-terminal and real-account acceptance remain separate.
