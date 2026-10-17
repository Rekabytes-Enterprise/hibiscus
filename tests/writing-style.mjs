import assert from 'node:assert/strict';
import writingStyle from '../src/pi/writing-style.mjs';
import approval from '../src/pi/approval.mjs';

const hooks = [];
writingStyle({ on(name, handler) {
  assert.equal(name, 'before_agent_start', 'writing guidance must not intercept tools');
  hooks.push(handler);
}});
assert.equal(hooks.length, 1);
const hook = hooks[0];
const options = { sections: { pi_base: 'Original safety and tools' }, selectedTools: ['read'] };
const event = { systemPromptOptions: options, prompt: 'Use a playful tone.' };
assert.equal(hook(event), undefined);
const guidance = options.sections.hibiscus_writing_style;
assert.match(guidance, /Follow the user's requested tone and format/);
assert.match(guidance, /Preserve exact quotations, code syntax, identifiers, commands/);
assert.match(guidance, /Preserve uncertainty/);
assert.match(guidance, /Report only actions and checks actually performed/);
assert.equal(options.sections.pi_base, 'Original safety and tools');
assert.deepEqual(options.selectedTools, ['read']);
assert.equal(event.prompt, 'Use a playful tone.');
hook(event);
assert.equal(options.sections.hibiscus_writing_style, guidance);

for (const sections of [undefined, null, [], 'invalid']) {
  const options = { sections, appendSystemPrompt: 'Existing appended guidance' };
  hook({ systemPromptOptions: options });
  assert.ok(options.appendSystemPrompt.startsWith('Existing appended guidance\n\n'));
  const once = options.appendSystemPrompt;
  hook({ systemPromptOptions: options });
  assert.equal(options.appendSystemPrompt, once);
}
assert.equal(hook({}), undefined);
assert.equal(hook(undefined), undefined);

// Pi applies hooks sequentially. Both registration orders must retain the
// other extension's prompt additions across modern and legacy hook shapes.
for (const extensions of [[approval, writingStyle], [writingStyle, approval]]) {
  const handlers = [];
  for (const extension of extensions) {
    extension({ on(name, handler) {
      if (name === 'before_agent_start') handlers.push(handler);
    }, registerTool() {} });
  }
  const structured = { systemPromptOptions: { sections: { base: 'Pi base' } } };
  const legacy = { systemPrompt: 'Pi base with safety and tool rules', prompt: 'Unchanged user request' };
  const optionsOnly = { systemPromptOptions: { appendSystemPrompt: 'Prior appended guidance' } };
  for (const input of [structured, legacy, optionsOnly]) {
    for (let repeat = 0; repeat < 2; repeat++) {
      for (const handler of handlers) {
        const result = handler(input);
        if (result?.systemPrompt !== undefined) input.systemPrompt = result.systemPrompt;
      }
    }
  }
  assert.equal(structured.systemPromptOptions.sections.base, 'Pi base');
  for (const name of ['hibiscus_identity', 'hibiscus_operational', 'hibiscus_writing_style']) {
    assert.ok(structured.systemPromptOptions.sections[name]);
    assert.equal(legacy.systemPrompt.split(`<${name}>`).length, 2);
    assert.equal(optionsOnly.systemPromptOptions.appendSystemPrompt.split(`<${name}>`).length, 2);
  }
  assert.ok(legacy.systemPrompt.startsWith('Pi base with safety and tool rules'));
  assert.equal(legacy.prompt, 'Unchanged user request');
  assert.ok(optionsOnly.systemPromptOptions.appendSystemPrompt.startsWith('Prior appended guidance'));
}
console.log('writing-style preservation and extension composition tests passed');
