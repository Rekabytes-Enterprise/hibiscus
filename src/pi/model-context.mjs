import { readFileSync } from 'node:fs';

// Explicit model IDs only: neither older Codex models nor future variants inherit
// a capability from their name. Keep this list aligned with official model docs.
const LONG_CONTEXT_MODELS = new Set([
  'gpt-5.4', 'gpt-5.5', 'gpt-5.6', 'gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6-luna',
  'gpt-6-sol', 'gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-luna',
]);

export function configureModelContext(pi, window) {
  if (window === null) return;
  pi.on('session_start', async (_event, ctx) => {
    const registry = ctx.modelRegistry;
    const models = registry.getAll().filter(model => model.provider === 'openai-codex');
    const eligible = model => LONG_CONTEXT_MODELS.has(model.id) && (!model.type || model.type === 'chat');
    if (models.some(model => eligible(model) && model.contextWindow !== window)) {
      // Public registry API preserves the built-in transport and authentication.
      // Copy all model metadata; only contextWindow changes for eligible models.
      registry.registerProvider('openai-codex', {
        models: models.map(model => eligible(model) ? { ...model, contextWindow: window } : model),
      });
      await registry.refresh({ allowNetwork: false });
    }
    // The agent can already hold a model object selected before session_start.
    // Re-select the registry object so SDK compaction and RPC usage agree.
    if (ctx.model?.provider === 'openai-codex' && eligible(ctx.model)) {
      const updated = registry.find(ctx.model.provider, ctx.model.id);
      if (updated && updated.contextWindow !== ctx.model.contextWindow) {
        if (!await pi.setModel(updated)) throw new Error('Could not apply Hibiscus Codex context setting to the active model');
      }
    }
  });
}

export default function modelContext(pi) {
  const config = JSON.parse(readFileSync(new URL('./context-config.json', import.meta.url), 'utf8'));
  configureModelContext(pi, config.codexContextWindow);
}
