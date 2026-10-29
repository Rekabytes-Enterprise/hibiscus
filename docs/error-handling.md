# Error handling and recovery

Interactive failures keep the composer and session available. One-shot and piped failures return nonzero. Initial startup errors and terminal I/O failures can still exit because a usable interface could not be established.

Provider HTTP errors expose the status and general recovery guidance, never the raw response body. Streaming errors or missing completion markers cannot execute partial tool calls. Tool failures become paired tool results so the model can inspect the outcome; a failed edit does not show a success diff. Error categories guide presentation and never authorize a retry or tool execution.

`/restore` returns the most recent failed prompt and attachments to the composer for review. It does not send them. `/reconnect` restarts the local runtime from the last confirmed native session; it does not resend a request. Tools may have changed the workspace before interruption, so inspect it before retrying.

Esc cancels the active provider future or shell group, drains cancellation acknowledgements and waits for settlement. Queued messages are cleared rather than silently replayed. A missing tool result on session resume is represented as unknown outcome and is not automatically executed.

`/login` and `/logout` stay inside Hibiscus and need fullscreen input. Invalid/missing keys or provider access errors can be corrected before retrying. Environment credentials remain available after removing a stored key. A successful local key save does not prove provider acceptance. Old Pi browser login and credential stores are not used.

Compaction failure/cancellation leaves active history unchanged. Successful compaction archives the prior snapshot privately. Unknown usage/context/pricing remains unknown. See [native migration](native-runtime.md) and `tests/native_runtime.rs` for the current contracts and test boundaries.
