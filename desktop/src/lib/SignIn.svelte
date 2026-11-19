<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { onRekaLogin, rekaCancel, rekaPaste, rekaStart } from "./engine";
  import Icon from "./Icon.svelte";

  // Covers the app instead of replacing it, so a draft in the message box survives (#185).
  let { onsignedin, onskip }: { onsignedin: () => void; onskip: () => void } = $props();

  /** The sign-in in progress: its link, and a pasted redirect URL. */
  let browser = $state<{ url: string; pasted: string; sent: boolean } | null>(null);
  let starting = $state(false);
  let copied = $state(false);
  let error = $state<string | null>(null);

  const message = (e: unknown) => String(e instanceof Error ? e.message : e);

  let stop: (() => void) | undefined;
  onMount(async () => {
    stop = await onRekaLogin((failure) => {
      if (!browser) return;
      browser = null;
      if (failure) error = failure;
      else onsignedin();
    });
  });
  onDestroy(() => {
    stop?.();
    if (browser) void rekaCancel();
  });

  async function start() {
    error = null;
    copied = false;
    starting = true;
    try {
      browser = { url: await rekaStart(), pasted: "", sent: false };
    } catch (e) {
      error = message(e);
    } finally {
      starting = false;
    }
  }

  async function paste() {
    if (!browser) return;
    try {
      await rekaPaste(browser.pasted.trim());
      browser.sent = true;
    } catch (e) {
      error = message(e);
    }
  }

  async function cancel() {
    browser = null;
    await rekaCancel().catch(() => {});
  }

  async function copy() {
    if (!browser) return;
    await navigator.clipboard.writeText(browser.url);
    copied = true;
  }
</script>

<div class="signin" data-tauri-drag-region>
  <div class="box">
    <div class="brandmark">Hibiscus</div>
    <p class="muted">Sign in with your Reka Bytes account to use Rooxcode models. It's the same account as the Rooxcode website, where you can see your usage and devices.</p>
    {#if browser}
      <form onsubmit={(e) => { e.preventDefault(); paste(); }}>
        <p class="small">{browser.sent ? "Finishing sign-in…" : "Waiting for the browser. Finish signing in there."}</p>
        <input aria-label="Redirect URL" type="text" autocomplete="off" spellcheck="false" placeholder="If the browser can't reach Hibiscus, paste the URL it ended on" bind:value={browser.pasted} disabled={browser.sent} />
        <div class="actions">
          <button type="button" class="link small" onclick={copy}>{copied ? "Link copied" : "Copy sign-in link"}</button>
          <span class="grow"></span>
          <button type="button" onclick={cancel}>Cancel</button>
          <button class="primary" disabled={browser.sent || !browser.pasted.trim()}>Use URL</button>
        </div>
      </form>
    {:else}
      <button class="primary wide" onclick={start} disabled={starting}><Icon name="log-in" /> Sign in with Reka Bytes</button>
      <button class="wide" onclick={onskip}>Continue with my own provider</button>
    {/if}
    {#if error}<p class="error small">{error}</p>{/if}
  </div>
</div>

<style>
  .signin { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; background: var(--bg); padding: 24px; }
  .box { width: min(380px, 100%); display: flex; flex-direction: column; gap: 12px; text-align: center; }
  .box form { display: flex; flex-direction: column; gap: 8px; text-align: left; }
  .wide { display: inline-flex; align-items: center; justify-content: center; gap: 8px; padding: 8px 12px; }
</style>
