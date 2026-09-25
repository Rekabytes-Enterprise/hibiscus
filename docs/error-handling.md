# Error handling and recovery

Hibiscus separates a **failed request** from a **broken Pi connection**. A model error should not close interactive chat. One-shot and piped commands still exit nonzero on failure so scripts can detect errors.

This mapping follows Pi 0.87.1's RPC records. Providers and extensions can introduce new error strings; this is an inventory of error channels, not an exhaustive list of provider messages.

## Error channels

| Pi signal / local failure | Handling |
| --- | --- |
| Matching `response` with `success: false` | Report a rejected command/preflight failure; keep chat open. Do not wait for settlement of an unaccepted prompt. Refresh authoritative session state after a failed command. |
| Assistant `message_end` with `stopReason: error` | Record the attempt's safe error details. Await Pi's recovery and `agent_settled`, not just `agent_end`. |
| Assistant `stopReason: aborted` | User Esc is cancellation, not failure. Unexpected aborts are recoverable failed runs. |
| Provider-stream `message_update` / `error` | Record provisional failure; a final assistant message can supersede it. Normal Pi runs report the authoritative `message_end`. |
| `auto_retry_start` | Show attempt/budget and delay. Pi owns backoff and retry policy; Esc remains available. |
| `auto_retry_end` | Success clears the failed attempt. Failure retains `finalError`. Neither event substitutes for `agent_settled`. |
| `compaction_start/end` | Show progress. Failed overflow recovery fails the run; threshold/manual compaction failures are warnings. Successful recovery or a subsequent successful assistant message clears the corresponding failure. |
| `summarization_retry_scheduled/attempt_start/finished` | Show retry progress. `finished` alone is not proof of success. |
| `tool_execution_end.isError` | Show the existing failed-tool status; do not fail the whole agent run. Pi can recover or choose another tool. |
| `extension_error` | Show a bounded warning without terminating the run. |
| Extension UI error notification | Show the notification, not a fatal application exception. |
| Cancelled session command | Keep the current chat; do not claim the session was changed. |
| RPC EOF, broken pipe, child exit | Enter a disconnected state; the last operation may have partially executed. |
| Invalid JSON, invalid required envelope fields, rejected cancellation/framing | Treat the connection as untrusted and disconnect. Unknown event types remain forward-compatible and are ignored. |
| RPC metadata/command request without a response for 30 seconds | Disconnect rather than waiting indefinitely; time spent in a human dialog does not consume the following response wait. This deadline is not a timeout on model generation. |
| Esc cancellation does not settle within 10 seconds | Disconnect and terminate the old Pi group; the operation's outcome is uncertain. Drain cancellation acknowledgements even if prompt preflight was rejected, so a delayed abort cannot affect the next prompt. |
| Initial Pi startup or terminal input/output failure | Can still exit: a usable UI/backend could not be established, or the terminal is no longer usable. |

## Rust boundary

- `src/pi/error.rs`: `ChatError`, `ErrorSource`, `ErrorCategory`, `RecoveryAction`, and bounded/redacted error presentation.
- `src/pi/run.rs`: `RunState` and `RunOutcome` track the **final** run result. A failed attempt is not a sticky failure when Pi later succeeds.
- `src/pi/rpc.rs`: preserve response IDs, strict JSONL framing, stderr separation, cancellation acknowledgements, and settlement semantics. Transport/protocol failures are typed separately from provider failures.
- `src/chat/mod.rs`: handle recoverable failures per interaction, refresh session status, retain the last failed prompt, and expose explicit recovery actions.
- `src/tui/screen.rs`: keep the transcript/composer available and display a disconnected footer when necessary.

Provider text is classified into authentication, access, billing/subscription/quota, temporary rate limit, provider unavailability, network, context limit, invalid/unsupported request, cancellation, or unknown. Classification is **advisory**: it chooses guidance, never whether Pi should retry or whether a connection is safe. Billing/quota patterns take precedence over `429`; a quota exhaustion response must not be presented as transient throttling.

Visible details strip control characters, have a length bound, and redact URLs/common credential forms. This is best-effort sanitization, not a guarantee that arbitrary provider text contains no sensitive data. Avoid publishing error screenshots or raw Pi diagnostics without reviewing them.

## Recovery commands

### `/restore`

Restores the last failed or disconnected-unsent prompt and its image attachments into the full-screen composer **for review**. It does not send the prompt. Enter sends only after you choose to do so; tools from the earlier attempt may already have changed files or performed other actions.

The recovery draft stays only in process memory. A later successful prompt or successful `/new` clears it. A newer failed/unsent prompt replaces it. Line mode reports that draft restoration requires the full-screen composer. Ordinary typing is never overwritten automatically.

### `/reconnect`

Available after disconnection. Terminates/reaps the old Pi process group before opening a replacement, using the last confirmed session path. A successful session switch updates that checkpoint immediately; new-session creation clears the old checkpoint. Reconnection does **not** resend prompts or replay tools.

If no session path was confirmed, Hibiscus explains that reconnecting opens a new Pi session; saved sessions can still be selected through `/sessions`. If reconnection/state synchronization fails, the UI remains disconnected and the command can be retried. While disconnected, `/help`, `/restore`, and `/quit` also remain usable; other input is not sent.

## Regression coverage

`tests/error_recovery.rs` exercises preflight rejection, subscription/quota failures, rate-limit exhaustion and successful retry, failed model commands, compaction failure and successful overflow recovery, warning-only tool/extension errors, Esc during retry, image/text restoration, backend crashes, malformed records, explicit same-session reconnect, failed reconnect, clean disconnected exit, and nonzero one-shot/piped exits. Unit tests cover classification/redaction, retry/compaction outcome transitions, and restored image drafts. These are mock Pi tests; real provider billing, rate limiting, and macOS recovery still need smoke tests.
