<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import Icon from "./Icon.svelte";
  import { offered, status, type Progress } from "./update";

  /** Finds a newer published release and installs it in place (#158). */
  let { busy, onrestart }: { busy: boolean; onrestart: () => Promise<void> } = $props();

  const DISMISSED = "hibiscus.updateDismissed";
  const EVERY = 6 * 60 * 60 * 1000;

  let update = $state<Update | null>(null);
  let progress = $state<Progress>({ phase: "available" });
  const working = $derived(progress.phase === "downloading" || progress.phase === "installing");

  // Offline, a GitHub error or no update: nothing to show.
  async function look() {
    if (working) return;
    try {
      const found = await check();
      if (found && offered(found.version, localStorage.getItem(DISMISSED))) {
        update = found;
        progress = { phase: "available" };
      }
    } catch {}
  }

  onMount(() => {
    void look();
    const timer = setInterval(look, EVERY);
    return () => clearInterval(timer);
  });

  async function install() {
    if (!update) return;
    let total: number | null = null;
    let done = 0;
    progress = { phase: "downloading", done, total };
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? null;
        if (e.event === "Progress") done += e.data.chunkLength;
        progress = e.event === "Finished" ? { phase: "installing" } : { phase: "downloading", done, total };
      });
      progress = { phase: "installing" };
      // Stop the engine and its tools before the app restarts into the new version.
      await onrestart();
      await relaunch();
    } catch (e) {
      progress = { phase: "failed", error: String(e instanceof Error ? e.message : e) };
    }
  }

  function dismiss() {
    if (update) localStorage.setItem(DISMISSED, update.version);
    update = null;
  }
</script>

{#if update}
  <div class="banner update" role="status">
    <span class="grow">{status(update.version, progress)}</span>
    {#if progress.phase === "available" || progress.phase === "failed"}
      <button class="primary" onclick={install} disabled={busy} title={busy ? "Wait for the current turn to finish" : undefined}><Icon name="download" size={14} /> Update</button>
    {/if}
    {#if !working}
      <button class="link" onclick={() => openUrl(`https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v${update!.version}`).catch(() => {})}>View release</button>
      <button class="iconbtn" onclick={dismiss} title="Dismiss" aria-label="Dismiss update"><Icon name="x" size={14} /></button>
    {/if}
  </div>
{/if}
