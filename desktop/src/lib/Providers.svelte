<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount } from "svelte";
  import {
    codexCancel,
    codexPaste,
    codexStart,
    listProviders,
    onCodexLogin,
    rekaAccount,
    saveApiKey,
    signOut,
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

  async function remove(p: ProviderInfo) {
    error = null;
    try {
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
        <Icon name="key" />
        <span class="grow">{p.id}</span>
        <span class="small" class:ok={p.signedIn} class:muted={!p.signedIn}>{p.signedIn ? "Signed in" : "Not signed in"}</span>
        {#if p.signedIn}
          <button onclick={() => remove(p)}>Sign out</button>
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
</ul>
{#if error}<p class="error small">{error}</p>{/if}
