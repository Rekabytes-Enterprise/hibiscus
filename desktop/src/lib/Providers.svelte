<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount } from "svelte";
  import {
    addLocalProvider,
    removeLocalProvider,
    codexCancel,
    codexPaste,
    codexStart,
    listProviders,
    onCodexLogin,
    rekaAccount,
    saveApiKey,
    signOut,
    type LocalProvider,
    type ProviderInfo,
    type RekaAccount,
  } from "./engine";
  import Icon from "./Icon.svelte";

  // onsignin shows the Reka Bytes sign-in screen.
  let { onsaved, onsignin }: { onsaved: () => Promise<void>; onsignin: () => void } = $props();

  let reka = $state<RekaAccount | null>(null);

  let providers = $state<ProviderInfo[]>([]);
  let chosen = $state<string | null>(null);
  let key = $state("");
  let error = $state<string | null>(null);
  let saving = $state(false);
  /** The Codex sign-in in progress: its link, and a pasted redirect URL. */
  let browser = $state<{ url: string; pasted: string; sent: boolean } | null>(null);
  let copied = $state(false);

  const message = (e: unknown) => String(e instanceof Error ? e.message : e);

  async function load() {
    try {
      // rooxcode is built in: nothing to set up.
      providers = (await listProviders()).filter((p) => p.apiKey || p.browser);
      reka = await rekaAccount();
    } catch (e) {
      error = message(e);
    }
  }

  let stop: (() => void) | undefined;
  onMount(async () => {
    stop = await onCodexLogin(async (failure) => {
      if (!browser) return;
      browser = null;
      if (failure) error = failure;
      else await changed();
    });
    await load();
  });
  onDestroy(() => {
    stop?.();
    if (browser) void codexCancel();
  });

  /** Engines read credentials when they start, so restart it after any change. */
  async function changed() {
    await onsaved();
    await load();
  }

  async function save() {
    if (!chosen) return;
    saving = true;
    error = null;
    try {
      await saveApiKey(chosen, key);
      key = "";
      chosen = null;
      await changed();
    } catch (e) {
      error = message(e);
    } finally {
      saving = false;
    }
  }

  async function startBrowser() {
    error = null;
    chosen = null;
    copied = false;
    try {
      browser = { url: await codexStart(), pasted: "", sent: false };
    } catch (e) {
      error = message(e);
    }
  }

  async function paste() {
    if (!browser) return;
    try {
      await codexPaste(browser.pasted.trim());
      browser.sent = true;
    } catch (e) {
      error = message(e);
    }
  }

  async function cancelBrowser() {
    browser = null;
    await codexCancel().catch(() => {});
  }

  async function copy() {
    if (!browser) return;
    await navigator.clipboard.writeText(browser.url);
    copied = true;
  }

  /** Signing out of Reka Bytes brings back the sign-in screen. */
  async function rekaSignOut() {
    error = null;
    try {
      await signOut("rooxcode");
      reka = null;
      onsignin();
    } catch (e) {
      error = message(e);
    }
  }

  /** The Add local AI form, while it is open. */
  let local = $state<{ id: string; baseUrl: string; api: LocalProvider["api"]; models: string; contextWindow: number | null; key: string } | null>(null);

  async function addLocal() {
    if (!local) return;
    saving = true;
    error = null;
    try {
      await addLocalProvider({
        id: local.id.trim(),
        baseUrl: local.baseUrl.trim(),
        api: local.api,
        models: local.models.split(/[\s,]+/).filter(Boolean),
        contextWindow: local.contextWindow || null,
        key: local.key,
      });
      local = null;
      await changed();
    } catch (e) {
      error = message(e);
    } finally {
      saving = false;
    }
  }

  async function remove(p: ProviderInfo) {
    error = null;
    try {
      if (p.local) {
        await removeLocalProvider(p.id);
        return await changed();
      }
      await signOut(p.id);
      await changed();
    } catch (e) {
      error = message(e);
    }
  }
</script>

<h2>Providers</h2>
<p class="muted small">Keys and logins are stored in Hibiscus's private files, the same ones the terminal app uses.</p>
<ul class="settings-list">
  <li>
    <div class="provider">
      <Icon name="log-in" />
      <span class="grow">Reka Bytes · Rooxcode models</span>
      <span class="small" class:ok={reka} class:muted={!reka}>{reka ? `Signed in${reka.email || reka.name ? ` as ${reka.email ?? reka.name}` : ""}` : "Not signed in"}</span>
      {#if reka}
        <button onclick={rekaSignOut}>Sign out</button>
      {:else}
        <button onclick={onsignin}>Sign in</button>
      {/if}
    </div>
  </li>
  {#each providers as p}
    <li>
      <div class="provider">
        <Icon name={p.local ? "server" : "key"} />
        <span class="grow">{p.id}{#if p.local}<span class="muted small"> · Local AI</span>{/if}</span>
        {#if p.local}
          <button onclick={() => remove(p)}>Remove</button>
        {:else}
          <span class="small" class:ok={p.signedIn} class:muted={!p.signedIn}>{p.signedIn ? "Signed in" : "Not signed in"}</span>
          {#if p.signedIn}
            <button onclick={() => remove(p)}>Sign out</button>
          {/if}
        {/if}
        {#if p.apiKey}
          <button onclick={() => { chosen = chosen === p.id ? null : p.id; key = ""; error = null; }}>
            {p.signedIn ? "Replace key" : "Add key"}
          </button>
        {:else if !p.signedIn && !browser}
          <button onclick={startBrowser}>Sign in</button>
        {/if}
      </div>
      {#if p.browser && browser}
        <form onsubmit={(e) => { e.preventDefault(); paste(); }}>
          <p class="small">{browser.sent ? "Finishing sign-in…" : "Waiting for the browser. Finish signing in there."}</p>
          <input aria-label="Redirect URL" type="text" autocomplete="off" spellcheck="false" placeholder="If the browser can't reach Hibiscus, paste the URL it ended on" bind:value={browser.pasted} disabled={browser.sent} />
          <div class="actions">
            <button type="button" class="link small" onclick={copy}>{copied ? "Link copied" : "Copy sign-in link"}</button>
            <span class="grow"></span>
            <button type="button" onclick={cancelBrowser}>Cancel</button>
            <button class="primary" disabled={browser.sent || !browser.pasted.trim()}>Use URL</button>
          </div>
        </form>
      {/if}
      {#if chosen === p.id}
        <form onsubmit={(e) => { e.preventDefault(); save(); }}>
          <!-- svelte-ignore a11y_autofocus -->
          <input aria-label={`${p.id} API key`} type="password" autocomplete="off" spellcheck="false" placeholder="Paste the API key" bind:value={key} autofocus />
          <div class="actions">
            {#if p.keyPage}
              <span class="small muted">Create one at <button type="button" class="link" onclick={() => openUrl(`https://${p.keyPage}`)}>{p.keyPage}</button></span>
            {/if}
            <span class="grow"></span>
            <button type="button" onclick={() => (chosen = null)}>Cancel</button>
            <button class="primary" disabled={saving || !key.trim()}>Save key</button>
          </div>
        </form>
      {/if}
    </li>
  {/each}
  <li>
    <div class="provider">
      <Icon name="server" />
      <span class="grow">Local AI<span class="muted small"> · Ollama, LM Studio, vLLM or your own server</span></span>
      {#if !local}
        <button onclick={() => { local = { id: "", baseUrl: "http://", api: "chat_completions", models: "", contextWindow: null, key: "" }; error = null; }}><Icon name="plus" size={14} /> Add local AI</button>
      {/if}
    </div>
    {#if local}
      <form class="local" onsubmit={(e) => { e.preventDefault(); addLocal(); }}>
        <label><span class="small">Name</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input type="text" autocomplete="off" spellcheck="false" placeholder="ornith" bind:value={local.id} autofocus />
        </label>
        <label><span class="small">Server URL</span>
          <input type="text" autocomplete="off" spellcheck="false" placeholder="http://192.168.1.20:8080/v1" bind:value={local.baseUrl} />
        </label>
        <label><span class="small">API</span>
          <select bind:value={local.api}>
            <option value="chat_completions">OpenAI Chat Completions (/chat/completions)</option>
            <option value="responses">OpenAI Responses (/responses)</option>
            <option value="anthropic">Anthropic Messages (/messages)</option>
          </select>
        </label>
        <label><span class="small">Models, separated by commas</span>
          <input type="text" autocomplete="off" spellcheck="false" placeholder="qwen3-coder-30b" bind:value={local.models} />
        </label>
        <label><span class="small">Context window in tokens (optional)</span>
          <input type="number" min="8192" placeholder="32768" bind:value={local.contextWindow} />
        </label>
        <label><span class="small">API key (optional)</span>
          <input type="password" autocomplete="off" spellcheck="false" placeholder="Leave empty if the server takes no key" bind:value={local.key} />
        </label>
        <p class="small muted">HTTP works for this computer and your private network: 192.168.x, 10.x, Tailscale, .local names. Anything else needs HTTPS.</p>
        <div class="actions">
          <span class="grow"></span>
          <button type="button" onclick={() => (local = null)}>Cancel</button>
          <button class="primary" disabled={saving || !local.id.trim() || !local.baseUrl.trim() || !local.models.trim()}>Add</button>
        </div>
      </form>
    {/if}
  </li>
</ul>
{#if error}<p class="error small">{error}</p>{/if}

<style>
  .local label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 8px;
  }
</style>
