# CLI and TUI structure

Hibiscus has two presentation implementations and one chat/runtime path. CLI owns normal-scrollback output and its editor. TUI owns the fullscreen transcript, composer, dialogs and painting. Shared terminal services and the frontend switcher live in `src/ui/`.

## Module map

```text
src/
├── main.rs                 Arguments and startup
├── chat/                   One chat loop and slash workflows
│   ├── mod.rs              Prompts, commands, recovery and view changes
│   ├── models.rs           Model/reasoning selection workflow
│   ├── sessions.rs         Workspace session browsing/history
│   ├── extension_commands.rs Local extension invocation
│   └── tool_outputs.rs     Saved-output query parsing
├── commands.rs             Shared native command catalog
├── cli/
│   ├── mod.rs              Exports the inline renderer/editor
│   ├── screen.rs           CLI streaming, waiting and tool lifecycle
│   ├── editor.rs           Bounded multiline draft editing
│   ├── style.rs            CLI colors and fragmented diff recognition
│   ├── tools.rs            Tool rows, previews and saved-output formatting
│   └── welcome.rs          Plain CLI welcome
├── tui/
│   ├── mod.rs              Exports the fullscreen renderer
│   ├── screen/
│   │   ├── mod.rs          Fullscreen state and session presentation
│   │   ├── input.rs        Composer, suggestions, selection and dialogs
│   │   ├── render.rs       Layout, cursor and frame painting
│   │   ├── events.rs       Runtime events, tools, activity and queues
│   │   └── tests.rs        Private-state regression tests
│   ├── activity.rs         Tool timeline
│   ├── paint.rs            Cached row painting
│   ├── markdown.rs         Formatted transcript layout
│   ├── mermaid.rs          Bounded diagram rendering
│   ├── modal.rs            Docked dialogs
│   ├── picker.rs           Picker/search state
│   ├── file_mentions.rs    Literal path completion
│   ├── clipboard.rs        Image clipboard jobs
│   ├── text_clipboard.rs   Transcript copying
│   └── header.rs           Persistent raspberry HIBI header
├── ui/
│   ├── frontend.rs         Active view, handover and presentation routing
│   ├── display.rs          Runtime display contracts and draft/queue data
│   ├── input.rs            Shared key/mouse decoding
│   ├── terminal.rs         One terminal reader, raw mode and dimensions
│   ├── paste.rs            Bounded atomic paste decoding
│   ├── mode.rs             CLI/TUI choice
│   ├── tool.rs             Decoded tool presentation data
│   ├── messages.rs         Text prompts, notices and help
│   ├── format.rs           Token count formatting
│   ├── text.rs             Clipboard encoding helper
│   └── welcome.rs          Welcome metadata and command hints
└── runtime/                Native agent, providers, tools and persistence
```

`tests/native_runtime.rs` retains the shared fixture and runtime integration tests. Its `native_runtime/cli.rs`, `tui.rs` and `presentation.rs` children cover each view and their shared behavior in the same integration binary. PTY support remains in `tests/support/`. Benchmarks point at their actual source modules.

## Caller's view

Startup uses `ui::Frontend::with_mode(interactive, Mode::Cli)` or `Frontend::new(interactive)` for the ordinary TUI default. The shared chat controller reads a fullscreen prompt through `prompt_with_update`, or an inline prompt through `read_inline`. Both paths submit to the same runtime client. Models, sessions and authentication workflows receive the neutral `Frontend`, rather than importing fullscreen internals.

`runtime::bridge` consumes `ui::display::ScrollDisplay` and decoded `ui::tool` data. The existing authentication/dialog adapters use neutral UI contracts. The native engine, providers, tools, socket protocol and persistence formats retain their existing ownership.

## Ownership and handover

`Frontend` owns exactly one active `View`, an enum containing either a boxed fullscreen screen or an inline screen. It retains the last runtime metadata snapshot, diagnostics feed, connection state and explicit recovery slots across presentation changes. Each concrete screen owns its own render state; CLI has no fullscreen painter, transcript layout or composer.

A view switch validates terminal capability, suspends the old frontend, creates the new frontend and restores shared metadata. It neither starts a runtime nor submits a prompt. The chat workflow restores visible history from the existing session when appropriate. CLI login/logout still temporarily use fullscreen native dialogs, then return inline. This refactor does not add inline authentication, completion or other feature parity.

Both renderers expose only their module entry points. Shared keyboard/paste/tool contracts contain no concrete renderer imports. CLI and TUI do not import each other. `ui::frontend` is the routing boundary that knows both implementations. This keeps ownership explicit per type-system discipline and avoids exposing fullscreen state to the inline editor.

The frontend interface hides terminal selection, cleanup and routing. It retains the existing broad display callback contract to avoid rewriting runtime event behavior during a structural change. Some methods still apply only to TUI, such as image staging and live queue input; CLI preserves its existing behavior for those callbacks.

## Alternatives considered

A folder-only move would put inline files under `cli/` but leave both renderers' state and keyboard/display interfaces inside the fullscreen screen. It would improve names without fixing ownership.

Separate crates and independent chat loops would enforce stronger compilation boundaries, but would introduce duplicated command/session behavior or a larger public API migration. They are unnecessary for the two existing views.

The selected design uses separate renderer state and neutral shared services within the existing binary. It keeps one session owner and one terminal reader, per boundary discipline. Mode-specific fallback/authentication behavior remains explicit in the controller. Shared metadata restoration and terminal cleanup are the main regression risks and are exercised through actual PTYs.

## Verification

Run formatting, the complete locked Rust tests and strict all-target Clippy. Presentation acceptance covers startup, CLI/TUI/CLI handover, native login returning inline, multiline/pasted prompts, colors, actual tools/diffs, saved output, queue recovery and terminal cleanup. Repeated handover must leave exactly one session, make no requests merely to switch views, and submit an atomic pasted draft once.

Provider protocols and OS-specific physical-terminal behavior retain their existing acceptance limits. This structural change adds no dependency or performance claim.

## Persistent TUI header

The TUI renders `header::rows(width, max_height, Header { info, thinking, session })` above every conversation frame. The header owns the raspberry slanted HIBI logo and version/model/thinking/workspace/session panel. It is never transcript data. Idle command/keyboard hints remain in the empty body and disappear after chat. CLI keeps its plain welcome.

Prepending a banner to conversation rows would let scrolling hide it and make selection/history ownership ambiguous. The chosen layout reserves the actual returned header height as chrome, then derives body capacity from terminal height, composer, queue and active dialog requirements. Short/narrow terminals use compact HIBI text; a small searchable picker or five-line draft keeps HIBI and thinking in one line. Header labels clip and pad by Unicode display width, including wide characters, joined emoji and combining marks. Screen stores the rendered transcript origin for mouse selection and modal cursor positioning. Paging and picker navigation use the available body rows.

Actual PTY regression coverage checks thinking updates, conversation/scroll, new/resumed sessions, short searchable pickers, multiline drafts with resize, Unicode label bounds, color/NO_COLOR and absence of header content in saved history. Thinking confirmations stay out of fullscreen chat; CLI uses native wording. Installer fixtures reject obsolete Pi-ready messaging.
