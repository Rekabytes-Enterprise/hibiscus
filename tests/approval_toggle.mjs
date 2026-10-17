import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

// Test the staged extension in an isolated directory, never put a bypass
// marker beside the real source or modify the user's Pi configuration.
const root = mkdtempSync(join(tmpdir(), 'hibiscus-approval-toggle-'));
try {
  const staged = join(root, 'approval.mjs');
  writeFileSync(staged, readFileSync(new URL('../src/pi/approval.mjs', import.meta.url)));
  const handlers = {};
  const tools = {};
  (await import(pathToFileURL(staged).href)).default({
    on(name, handler) { handlers[name] = handler; },
    registerTool(tool) { tools[tool.name] = tool; },
  });
  let prompts = 0;
  const ctx = { cwd: '/fixture', hasUI: true, ui: {
    async select() { prompts++; return 'Deny'; },
  }};
  const event = { toolName: 'bash', input: { command: 'rm -rf fixture' } };
  assert.equal((await handlers.tool_call(event, ctx)).block, true);
  assert.equal(prompts, 1);
  assert.deepEqual(Object.keys(tools), ['goal'], 'only the goal tool remains registered');
  const manualFlag = join(root, 'approval-disabled');
  writeFileSync(manualFlag, 'disabled');
  assert.equal(await handlers.tool_call(event, ctx), undefined);
  assert.equal(prompts, 1, 'manual disable must bypass approval');
  rmSync(manualFlag);
  assert.equal((await handlers.tool_call(event, ctx)).block, true);
  assert.equal(prompts, 2, 'manual enable must restore approval');
} finally {
  rmSync(root, { recursive: true, force: true });
}
console.log('isolated approval toggle tests passed');
