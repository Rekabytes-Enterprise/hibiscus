<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { rekaUsage, type RekaUsage } from "./engine";
  import Icon from "./Icon.svelte";

  let { retryAfter, ondismiss }: { retryAfter: number | null; ondismiss: () => void } = $props();
  const site = "https://rooxcode.reka-bytes.my";
  const observedAt = Date.now();
  let usage = $state<RekaUsage | null>(null);
  let linkError = $state(false);

  onMount(() => {
    let active = true;
    void rekaUsage().then((value) => { if (active) usage = value; }, () => {});
    return () => { active = false; };
  });

  const weekly = $derived(usage?.week?.percent === 100);
  const fiveHours = $derived(usage?.fiveHours?.percent === 100);
  const title = $derived(weekly && fiveHours ? "5-hour and weekly limits reached"
    : weekly ? "Weekly usage limit reached"
    : fiveHours ? "5-hour usage limit reached"
    : "Rooxcode usage limit reached");
  // Retry-After covers both full windows. dropsAt is when the oldest usage leaves a window.
  const windowTimes = $derived([
    weekly ? usage?.week?.dropsAt : null,
    fiveHours ? usage?.fiveHours?.dropsAt : null,
  ]);
  const refreshAt = $derived(retryAfter != null && retryAfter > 0 ? observedAt + retryAfter * 1000
    : Math.max(0, ...windowTimes.map((seconds) => typeof seconds === "number" && Number.isFinite(seconds) && seconds * 1000 > observedAt && seconds * 1000 <= observedAt + 604800000 ? seconds * 1000 : 0)));
  const time = $derived(refreshAt ? new Date(refreshAt).toLocaleString(undefined,
    { weekday: "short", hour: "numeric", minute: "2-digit" }) : null);

  async function moreUsage() {
    try {
      await openUrl(site);
      linkError = false;
    } catch {
      linkError = true;
    }
  }
</script>

<section class="usage-notice" aria-label="Rooxcode usage">
  <div class="usage-copy" role="status">
    <Icon name="alert-circle" size={16} />
    <div><strong>{title}</strong>
      {#if time}<span class="muted">Room again {time}</span>{/if}
      {#if linkError}<span class="muted">Open {site} in your browser.</span>{/if}
    </div>
  </div>
  <button class="plain more-usage" onclick={moreUsage}>Get more usage</button>
  <button class="iconbtn" aria-label="Dismiss usage notice" title="Dismiss" onclick={ondismiss}><Icon name="x" size={14} /></button>
</section>

<style>
  .usage-notice {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
    padding: 7px 8px 7px 12px;
    border: 1px solid var(--line);
    border-radius: 13px;
    background: var(--panel);
    font-size: 13px;
  }
  .usage-copy { display: flex; align-items: center; gap: 8px; flex: 1 1 240px; }
  .usage-copy div { display: flex; flex-wrap: wrap; gap: 2px 8px; }
  strong { font-weight: 500; }
  .more-usage { background: var(--selected); }
</style>
