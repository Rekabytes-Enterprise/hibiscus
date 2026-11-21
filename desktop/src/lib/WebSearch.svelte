<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { webSearch } from "./engine";
  import Icon from "./Icon.svelte";

  let status = $state<"key" | "environment" | null>(null);
  let loading = $state(true);
  let editing = $state(false);
  let saving = $state(false);
  let key = $state("");
  let error = $state<string | null>(null);

  function showError(failure: unknown) {
    error = failure instanceof Error ? failure.message : String(failure);
  }

  onMount(async () => {
    try {
      status = await webSearch.status();
    } catch (failure) {
      showError(failure);
    } finally {
      loading = false;
    }
  });

  async function save(action: "connect" | "disconnect") {
    if (saving) return;
    saving = true;
    error = null;
    try {
      if (action === "connect") {
        await webSearch.connect(key);
        status = "key";
        key = "";
        editing = false;
      } else {
        await webSearch.disconnect();
        status = await webSearch.status();
      }
    } catch (failure) {
      showError(failure);
    } finally {
      saving = false;
    }
  }
</script>

<li>
  <div class="provider">
    <Icon name="globe" />
    <span class="grow">Brave Search · Web search</span>
    <span class="small" class:ok={status} class:muted={!status}>
      {loading ? "Checking…" : status === "environment" ? "Using environment key" : status ? "Connected" : "Not connected"}
    </span>
    {#if !loading && !editing}
      {#if status === "key"}
        <button disabled={saving} onclick={() => save("disconnect")}>Disconnect</button>
      {:else if !status}
        <button class="primary" onclick={() => { editing = true; error = null; }}>Add API key</button>
      {/if}
    {/if}
  </div>
  {#if editing}
    <form onsubmit={(event) => { event.preventDefault(); void save("connect"); }}>
      <input aria-label="Brave Search API key" type="password" autocomplete="off" spellcheck="false" placeholder="Paste your Brave Search API key" bind:value={key} disabled={saving} />
      <div class="actions">
        <span class="small muted">Search queries go to Brave. <button type="button" class="link" onclick={() => openUrl("https://brave.com/search/api/")}>Get an API key</button></span>
        <span class="grow"></span>
        <button type="button" disabled={saving} onclick={() => { editing = false; key = ""; error = null; }}>Cancel</button>
        <button class="primary" disabled={saving || !key.trim()}>{saving ? "Checking…" : "Connect"}</button>
      </div>
    </form>
  {/if}
  {#if error}<p class="error small" role="status">{error}</p>{/if}
</li>
