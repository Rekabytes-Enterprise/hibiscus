<script lang="ts">
  import { onMount } from "svelte";
  import { request, type ModelInfo } from "./engine";

  let {
    current,
    onclose,
    onchosen,
  }: {
    current: { provider: string; id: string } | null;
    onclose: () => void;
    onchosen: () => void;
  } = $props();

  let models = $state<ModelInfo[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");
  // null = the All tab.
  let tab = $state<string | null>(null);

  const providers = $derived([...new Set(models.map((m) => m.provider))]);
  const shown = $derived(
    models.filter(
      (m) =>
        (tab === null || m.provider === tab) &&
        query
          .toLowerCase()
          .split(/\s+/)
          .every((term) => `${m.provider}/${m.id}`.toLowerCase().includes(term)),
    ),
  );

  onMount(async () => {
    try {
      const data = await request({ type: "get_available_models" });
      models = data.models ?? [];
      error = data.catalogError ?? null;
      tab = current && models.some((m) => m.provider === current.provider) ? current.provider : null;
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
    } finally {
      loading = false;
    }
  });

  async function choose(m: ModelInfo) {
    try {
      await request({ type: "set_model", provider: m.provider, modelId: m.id });
      onchosen();
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
    }
  }

  const size = (n: number | null) =>
    n == null ? "" : n >= 1_000_000 ? `${+(n / 1_048_576).toFixed(1)}M ctx` : `${Math.round(n / 1024)}K ctx`;
</script>

<div class="backdrop" role="presentation" onclick={onclose}>
  <div class="modal wide" role="dialog" aria-label="Models" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>Models</h2>
    <!-- svelte-ignore a11y_autofocus -->
    <input placeholder="Search, e.g. openrouter claude" bind:value={query} autofocus />
    <div class="tabs" role="tablist">
      {#each providers as p}
        <button role="tab" class:active={tab === p} onclick={() => (tab = p)}>{p} {models.filter((m) => m.provider === p).length}</button>
      {/each}
      <button role="tab" class:active={tab === null} onclick={() => (tab = null)}>All {models.length}</button>
    </div>
    {#if error}<p class="error small">{error}</p>{/if}
    <ul class="models">
      {#if loading}
        <li class="muted">Loading models…</li>
      {:else}
        {#each shown as m (m.provider + "/" + m.id)}
          <li>
            <button class:active={current?.provider === m.provider && current?.id === m.id} onclick={() => choose(m)}>
              <span>{tab === null ? `${m.provider}/` : ""}{m.id}</span>
              <span class="muted small">{[size(m.contextWindow), m.images && "img", m.reasoning && "reasoning"].filter(Boolean).join(" · ")}</span>
            </button>
          </li>
        {:else}
          <li class="muted">No models match. Signed-out providers are hidden; add a key under Providers.</li>
        {/each}
      {/if}
    </ul>
    <div class="actions"><span class="grow"></span><button onclick={onclose}>Close</button></div>
  </div>
</div>
