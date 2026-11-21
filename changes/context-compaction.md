### Fixed

- Limit inline tool output on small-context models and check context before sending tool results. Compact and retry once after an explicit provider context-limit rejection.
- Send non-Rooxcode compaction requests as bounded conversation data and keep the original history when summarization fails or returns an invalid result.
