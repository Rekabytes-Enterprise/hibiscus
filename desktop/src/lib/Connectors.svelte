<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy, onMount } from "svelte";
  import { elevenlabs } from "./engine";
  import Icon from "./Icon.svelte";

  /** Services the canvas uses besides the chat's model providers. */
  const KEY_PAGE = "elevenlabs.io/app/settings/api-keys";

  let status = $state<"oauth" | "key" | null>(null);
  /** The browser sign-in in progress: its link, and a pasted redirect URL. */
  let browser = $state<{ url: string; pasted: string; sent: boolean } | null>(null);
  let adding = $state(false);
  let key = $state("");
  let saving = $state(false);
  let error = $state<string | null>(null);

  const message = (e: unknown) => String(e instanceof Error ? e.message : e);
  const load = async () => (status = await elevenlabs.status().catch(() => null));

  let stop: (() => void) | undefined;
  onMount(async () => {
    stop = await elevenlabs.onLogin(async (failure) => {
      if (!browser) return;
      browser = null;
      if (failure) error = failure;
      await load();
    });
    await load();
  });
  onDestroy(() => {
    stop?.();
    if (browser) void elevenlabs.cancel();
  });

  async function signIn() {
    error = null;
    adding = false;
    try {
      browser = { url: await elevenlabs.signIn(), pasted: "", sent: false };
    } catch (e) {
      error = message(e);
    }
  }

  async function paste() {
    if (!browser) return;
    try {
      await elevenlabs.paste(browser.pasted.trim());
      browser.sent = true;
    } catch (e) {
      error = message(e);
    }
  }

  async function cancel() {
    browser = null;
    await elevenlabs.cancel().catch(() => {});
  }

  async function connect() {
    saving = true;
    error = null;
    try {
      await elevenlabs.connect(key);
      key = "";
      adding = false;
      await load();
    } catch (e) {
      error = message(e);
    } finally {
      saving = false;
    }
  }

  async function disconnect() {
    error = null;
    try {
      await elevenlabs.disconnect();
      await load();
    } catch (e) {
      error = message(e);
    }
  }
</script>

<h2>Connectors</h2>
<p class="muted small">Services the canvas uses to make audio, images and video. Sign-ins and keys are stored in Hibiscus's private files and sent only to that service.</p>
<ul class="settings-list">
  <li>
    <div class="provider">
      <Icon name="mic" />
      <span class="grow">ElevenLabs · Text to speech</span>
      <span class="small" class:ok={status} class:muted={!status}>{status === "oauth" ? "Signed in" : status === "key" ? "Connected with an API key" : "Not connected"}</span>
      {#if status}
        <button onclick={disconnect}>Disconnect</button>
      {:else if !browser}
        <button class="primary" onclick={signIn}>Sign in with ElevenLabs</button>
      {/if}
    </div>
    {#if browser}
      <form onsubmit={(e) => { e.preventDefault(); paste(); }}>
        <p class="small">{browser.sent ? "Finishing sign-in…" : "Waiting for the browser. Approve Hibiscus on the ElevenLabs page there."}</p>
        <input aria-label="Redirect URL" type="text" autocomplete="off" spellcheck="false" placeholder="If the browser can't reach Hibiscus, paste the URL it ended on" bind:value={browser.pasted} disabled={browser.sent} />
        <div class="actions">
          <button type="button" class="link small" onclick={() => openUrl(browser!.url)}>Open the sign-in page again</button>
          <span class="grow"></span>
          <button type="button" onclick={cancel}>Cancel</button>
          <button class="primary" disabled={browser.sent || !browser.pasted.trim()}>Use URL</button>
        </div>
      </form>
    {:else if !status}
      {#if adding}
        <form onsubmit={(e) => { e.preventDefault(); connect(); }}>
          <!-- svelte-ignore a11y_autofocus -->
          <input aria-label="ElevenLabs API key" type="password" autocomplete="off" spellcheck="false" placeholder="Paste the ElevenLabs API key" bind:value={key} autofocus />
          <div class="actions">
            <span class="small muted">Create one at <button type="button" class="link" onclick={() => openUrl(`https://${KEY_PAGE}`)}>{KEY_PAGE}</button>. It needs text to speech and voices access.</span>
            <span class="grow"></span>
            <button type="button" onclick={() => (adding = false)}>Cancel</button>
            <button class="primary" disabled={saving || !key.trim()}>{saving ? "Checking…" : "Connect"}</button>
          </div>
        </form>
      {:else}
        <p class="small muted alt">Have an API key? <button class="link" onclick={() => { adding = true; error = null; }}>Use an API key instead</button></p>
      {/if}
    {/if}
  </li>
</ul>
{#if error}<p class="error small">{error}</p>{/if}

<style>
  .alt { margin: 6px 0 0 26px; }
</style>
