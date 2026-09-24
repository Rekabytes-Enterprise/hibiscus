# Hibiscus agent instructions

These instructions apply when working in this repository. Read `.pi/STATE.md` and `.pi/MEMORY.md` before planning or editing; they are maintained project records, not substitutes for checking current code.

## Project rules

- Hibiscus is a Rust CLI that controls Pi via `pi --mode rpc` (JSONL over stdin/stdout). Keep Pi responsible for models, authentication, tools, and agent behavior. Do not implement a second agent without an explicit decision.
- Preserve strict JSONL framing and keep stderr separate from RPC stdout. Correlate command responses by ID; a successful prompt response means acceptance, not completion. Wait for `agent_settled` for completed runs.
- Make targeted changes and add regression tests for bugs when feasible. Never claim tests passed without running them; record missing tooling and other verification blockers explicitly.
- Do not put credentials, private session transcripts, or user-specific paths in committed records.

## Working records

- `.pi/STATE.md` is the current snapshot: status, next steps, and verification blockers. Update it when progress changes; replace stale items rather than appending a diary.
- `.pi/MEMORY.md` is durable, nonredundant knowledge: confirmed pitfalls, their causes/fixes, and decisions that affect future work. Consult it before re-solving a problem.
- Before adding a memory, search for an existing entry on the same underlying issue or decision. Update that entry rather than adding a duplicate; merge overlapping entries and remove obsolete ones. Record one canonical fix per root cause, not one entry per symptom or recurrence. Link to a regression test when present.
- Only record verified facts as facts. Mark unverified ideas as hypotheses in STATE, not MEMORY. If a memory is disproved, correct or delete it. Keep progress updates out of MEMORY and historical chatter out of STATE.
- At the end of a task, reconcile both records with the code and test results. Leave untouched when nothing relevant changed.
