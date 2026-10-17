import assert from 'node:assert/strict';
import { configureModelContext } from '../src/pi/model-context.mjs';

const model = (id, provider = 'openai-codex') => ({
  id, name: id, provider, api: 'openai-codex-responses',
  baseUrl: 'https://example.invalid', contextWindow: 272000, maxTokens: 128000,
  reasoning: true, input: ['text'], cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0 },
});
const originals = [model('gpt-6.1-sol'), model('gpt-5.3-codex'), model('gpt-6.1-sol-mini'), model('gpt-6.1-sol', 'openai')];
let catalog = structuredClone(originals), selected = catalog[0], registrations = 0;
let hook;
const registry = {
  getAll: () => catalog,
  registerProvider(provider, config) {
    assert.equal(provider, 'openai-codex');
    assert.deepEqual(Object.keys(config), ['models'], 'do not replace auth or transport');
    registrations++;
    catalog = [...catalog.filter(m => m.provider !== provider), ...config.models];
  },
  async refresh(options) { assert.equal(options.allowNetwork, false); },
  find: (provider, id) => catalog.find(m => m.provider === provider && m.id === id),
};
configureModelContext({
  on: (event, handler) => { assert.equal(event, 'session_start'); hook = handler; },
  setModel: async m => { selected = m; return true; },
}, 1000000);
await hook({}, { modelRegistry: registry, model: selected });
assert.equal(selected.contextWindow, 1000000);
assert.deepEqual(selected, { ...originals[0], contextWindow: 1000000 });
for (const untouched of originals.slice(1)) assert.deepEqual(registry.find(untouched.provider, untouched.id), untouched);
await hook({}, { modelRegistry: registry, model: originals[0] });
assert.equal(registrations, 1, 'session switch must not repeatedly replace the catalog');
assert.equal(selected.contextWindow, 1000000, 'stale active object must be updated even when catalog is current');
configureModelContext({ on() { assert.fail('disabled override must not register hooks'); } }, null);

// Optional installed-SDK check. Uses an in-memory credential store, no accounts,
// no provider requests, and no user configuration files.
if (process.argv[2]) {
  const root = new URL(`file://${process.argv[2].replace(/\/$/, '')}/`);
  const { ModelRuntime } = await import(new URL('core/model-runtime.js', root));
  const { ModelRegistry } = await import(new URL('core/model-registry.js', root));
  const { AuthStorage } = await import(new URL('core/auth-storage.js', root));
  const { shouldCompact } = await import(new URL('core/compaction/compaction.js', root));
  process.env.PI_OFFLINE = '1';
  const runtime = await ModelRuntime.create({ credentials: AuthStorage.inMemory(), modelsPath: null, refreshOnCreate: false });
  const actual = new ModelRegistry(runtime);
  actual.registerProvider('openai-codex', { models: originals.filter(m => m.provider === 'openai-codex') });
  const before = actual.getProvider('openai-codex');
  let active = actual.find('openai-codex', 'gpt-6.1-sol');
  configureModelContext({
    on: (_event, handler) => { hook = handler; },
    setModel: async m => { active = m; return true; },
  }, 1000000);
  await hook({}, { modelRegistry: actual, model: active });
  assert.equal(active.contextWindow, 1000000);
  assert.equal(actual.find('openai-codex', 'gpt-5.3-codex').contextWindow, 272000);
  assert.equal(typeof actual.getProvider('openai-codex').auth.oauth.toAuth, typeof before.auth.oauth.toAuth);
  const settings = { enabled: true, reserveTokens: 16384 };
  assert.equal(shouldCompact(300000, 272000, settings), true);
  assert.equal(shouldCompact(300000, active.contextWindow, settings), false);
  assert.equal(shouldCompact(990000, active.contextWindow, settings), true);
  console.log('installed Pi registry and compaction checks passed (offline)');
}
console.log('model context overrides passed');
