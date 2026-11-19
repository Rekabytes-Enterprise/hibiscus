<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { rekaUsage, type RekaAccount, type RekaUsage, type RekaWindow } from "./engine";
  import Icon from "./Icon.svelte";
  import PixelAvatar from "./PixelAvatar.svelte";

  // `account` is the saved profile, shown while the page loads fresh usage from Rooxcode.
  let {
    account,
    onsignin,
    onsignout,
    onloaded,
  }: {
    account: RekaAccount | null;
    onsignin: () => void;
    onsignout: () => void;
    /** Hands the fresh profile back, so the sidebar follows a new name or avatar. */
    onloaded: (account: RekaAccount) => void;
  } = $props();

  const SITE = "https://rooxcode.com/account";
  let usage = $state<RekaUsage | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    if (!account) return;
    loading = true;
    error = null;
    try {
      usage = await rekaUsage();
      onloaded(usage.account);
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  const profile = $derived(usage?.account ?? account);
  const windows = $derived<[string, RekaWindow | null][]>(
    usage ? [["5-hour limit", usage.fiveHours], ["Weekly limit", usage.week]] : [],
  );
  const time = (seconds: number) =>
    new Date(seconds * 1000).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" });
  const wait = (seconds: number) => {
    const minutes = Math.ceil(seconds / 60);
    return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
  };
  function note(w: RekaWindow) {
    if (w.percent >= 100 && w.retryAfter != null) return `Full. Room again in ${wait(w.retryAfter)}.`;
    if (w.percent > 0 && w.dropsAt != null) return `Next refresh at ${time(w.dropsAt)}.`;
    return "Nothing used yet.";
  }
  const open = (path = "") => void openUrl(SITE + path).catch(() => {});
</script>

<h2>Account</h2>
{#if !profile}
  <p class="muted">Sign in with Reka Bytes to use Rooxcode models and see your usage here.</p>
  <button onclick={onsignin}><Icon name="log-in" size={14} /> Sign in with Reka Bytes</button>
{:else}
  <div class="account-head">
    <span class="account-avatar"><PixelAvatar id={profile.avatar} size={56} /></span>
    <div class="grow">
      <div class="account-name">{profile.name || "Reka Bytes account"}</div>
      {#if profile.email}<div class="muted small">{profile.email}</div>{/if}
      {#if usage?.device}<div class="muted small">This device: {usage.device}</div>{/if}
    </div>
    <button class="plain" onclick={() => open("/profile")}><Icon name="arrow-up-right" size={14} /> Change avatar</button>
  </div>

  <h3>Usage</h3>
  {#if error}
    <div class="banner error">{error} <button class="link" onclick={load}>Try again</button></div>
  {:else if !usage}
    <p class="muted small">{loading ? "Loading your usage…" : ""}</p>
  {:else}
    <ul class="settings-list">
      {#each windows as [label, w]}
        <li>
          <div class="usage-row">
            <span class="grow">{label}</span>
            <span class="muted small">{w ? `${w.percent}% used` : "No limit"}</span>
          </div>
          {#if w}
            <div class="usage-bar" class:full={w.percent >= 100}><span style="width: {w.percent}%"></span></div>
            <div class="muted small">{note(w)}</div>
          {/if}
        </li>
      {/each}
      {#if usage.credits != null}
        <li>
          <div class="usage-row">
            <span class="grow">Wallet</span>
            <span>{usage.credits.toLocaleString()} credits</span>
          </div>
          <div class="muted small">Credits keep requests going after a limit is full.</div>
        </li>
      {/if}
    </ul>
  {/if}

  <div class="account-actions">
    <button class="plain" onclick={() => open("/dashboard")}><Icon name="arrow-up-right" size={14} /> Open account website</button>
    <button class="plain" onclick={onsignout}><Icon name="log-out" size={14} /> Sign out</button>
  </div>
{/if}
