import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

// Test the staged extension in an isolated directory, never put a bypass
// marker beside the real source or modify the user's Pi configuration.
const root = mkdtempSync(join(tmpdir(), 'hibiscus-loop-approval-'));
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
  writeFileSync(join(root, 'loop-active'), 'active');
  assert.equal((await tools.loop_status.execute('s', {action:'candidate_complete'})).details.hibiscusLoop.state, 'candidate_complete');
  assert.match((await tools.loop_status.execute('s', {action:'submit_evidence'})).details.hibiscusLoop.error, /projectDir/);
  const validated = await tools.loop_status.execute('s', {action:'submit_evidence',projectDir:'app',reportPath:'report.json'}, undefined, undefined,
    {ui:{async input(title, payload) {
      assert.equal(title,'Hibiscus internal loop evidence');
      assert.equal(JSON.parse(payload).reportPath,'report.json');
      return JSON.stringify({accepted:false,verified:2,total:3,missing:['Delete console attachment']});
    }}});
  assert.deepEqual(validated.details.hibiscusLoop,{state:'submit_evidence',validatedByClient:true});
  assert.equal(JSON.parse(validated.content[0].text).verified,2);
  assert.match((await tools.loop_status.execute('s', {action:'blocked'})).details.hibiscusLoop.error, /reason/);
  assert.equal((await tools.loop_status.execute('s', {action:'blocked',reason:'No browser available'})).details.hibiscusLoop.reason, 'No browser available');
  assert.equal(await handlers.tool_call(event, ctx), undefined);
  assert.equal(prompts, 1, 'active loop must not open a human approval dialog');
  rmSync(join(root, 'loop-active'));
  assert.match((await tools.loop_status.execute('s', {action:'candidate_complete'})).details.hibiscusLoop.error, /not active/);
  assert.equal((await handlers.tool_call(event, ctx)).block, true);
  assert.equal(prompts, 2, 'approval must resume when the scoped bypass ends');
} finally {
  rmSync(root, { recursive: true, force: true });
}
console.log('isolated loop approval toggle tests passed');
