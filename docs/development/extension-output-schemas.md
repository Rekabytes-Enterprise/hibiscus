# Extension output schemas

## Problem and usage

An extension reply has a checked envelope, but its optional `data` has no declared
contract. A tool can now declare `outputSchema`, a self-contained Draft 7 object
or boolean schema. Omission preserves existing behavior. A successful reply is
validated against that schema; omitted data is JSON null. Logical failures and
slash commands retain their existing contracts.

## Shape

```text
manifest Tool { parameters: Value, output_schema: Option<Value>, ... }
ToolSchemas { input: Arc<Validator>, output: Option<Arc<Validator>> }
Registry Tool { schemas: ToolSchemas, ... }
manifest::parse(bytes) -> (Manifest, Vec<ToolSchemas>)
Registry::execute_tool(...) -> Result<Value>
```

Manifest parsing owns compilation with the existing offline Draft 7 validator
and linear-time regex engine. Each registry snapshot owns both validators for a
tool. Invocation decodes and correlates the bounded reply first, then validates
successful data before constructing a tool result. Invalid output produces an
error result with the known zero OS exit code and extension ID. Neither invalid
data nor its success text enters the model or session. The engine already owns
call IDs, duration, persistence and continued provider execution.

## Synthesis decision

Two candidate shapes were compared sequentially. The chosen design pairs input
and output validators at the existing manifest boundary and checks data in the
registry that owns the selected tool. This shares the same behavior with managed
update checks and keeps transport unaware of tool-specific schemas.

The alternative moved schema lookup and validation into the process transport,
returning a new invocation outcome carrying OS status and schema errors. It would
make commands and envelope decoding coordinate metadata that only tools need.
The existing registry result already preserves exit zero on schema failures;
nonzero exits retain the existing typed process error. No new transport outcome
or parallel validator list is needed.

## Tradeoffs

Output schemas remain local contracts rather than provider tool-definition
fields, since supported providers have different wire schemas. This preserves
current provider catalogs. Logical error replies are exempt so a failure need
not fabricate success-shaped data. Schema diagnostics use fixed text and never
format rejected values. Explicit null is not an omitted schema. Existing size,
timeout, cancellation and version snapshot policies continue to apply.

## Verification

Unit tests cover compilation, offline references, boolean and null semantics,
valid and invalid outputs, commands and logical errors. Native integration tests
exercise real subprocesses and local synthetic provider requests, persisted
results, call correlation and recovery. Existing version tests cover snapshot
activation and rollback. No real provider credentials are needed.
