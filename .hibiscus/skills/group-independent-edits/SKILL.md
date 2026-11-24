---
name: group-independent-edits
description: Batch ready, independent Hibiscus native replacements during a multi-file refactor. Use when several exact replacements are known from inspected files and can be applied without seeing another edit's result first.
---

# Group independent edits

Read the files you need to plan the change. If you still need this skill and several file reads, request them in the same model response.

Once two or more replacements are ready, send one `batch_edit` call. Each replacement carries its own `path`, `oldText`, and `newText`. Use unique, non-overlapping matches in the original files. Hibiscus validates the replacements together, writes each changed file once, and records a diff for each file.

Keep dependent calls in separate responses. If an edit changes text another edit matches, or a later edit depends on a test or tool outcome, inspect the result before issuing that next call.

Review the results before verification. A replacement that fails validation leaves the batch untouched. If a write fails after some files were saved, inspect the tool result and checkpoint before retrying only the remaining work. Run the relevant checks after the edits settle and report what passed.
