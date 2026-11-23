---
name: group-independent-edits
description: Group ready, independent Hibiscus native edit calls during a multi-file refactor. Use when several exact replacements are known from inspected files and can be applied without seeing another edit's result first.
---

# Group independent edits

Read the files you need to plan the change. If you still need this skill and several file reads, request them in the same model response.

Once each replacement is ready, emit the independent `edit` calls together in one model response. Each call still carries its own `path`, `oldText`, and `newText`. Use the smallest unambiguous match from the file you inspected. Hibiscus runs the calls in order and records their individual results and diffs.

Keep dependent calls in separate responses. If an edit changes text another edit matches, or a later edit depends on a test or tool outcome, inspect the result before issuing that next call.

Review the results before verification. If one edit fails, keep the successful edits, re-read the failed file, and retry only the change that still needs to land. Run the relevant checks after the edits settle and report what passed.
