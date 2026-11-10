<script lang="ts">
  import Icon from "./Icon.svelte";
  import { name } from "./paths";

  let {
    folder,
    recent,
    busy,
    onopen,
    onchoose,
    onremove,
    onproviders,
  }: {
    folder: string | null;
    recent: string[];
    busy: boolean;
    onopen: (path: string) => void;
    onchoose: () => void;
    /** Takes a folder out of the list; nothing on disk changes. */
    onremove: (path: string) => void;
    /** Set while no provider is signed in. */
    onproviders?: () => void;
  } = $props();

</script>

<div class="start">
  <div class="brandmark">Hibiscus</div>
  {#if onproviders}
    <div class="card">
      <button class="row" onclick={onproviders}>
        <span class="label"><Icon name="key" /> Add a provider to start</span>
        <span class="muted"><Icon name="arrow-right" /></span>
      </button>
    </div>
  {/if}
  <div class="card">
    {#each folder ? [folder, ...recent.filter((p) => p !== folder).slice(0, 3)] : [] as path}
      <div class="folderrow">
        <button class="row" onclick={() => onopen(path)} disabled={busy || path === folder} title={path}>
          <span class="label"><Icon name="folder" /> {name(path)}</span>
          {#if path === folder}<span class="check"><Icon name="check" /></span>{/if}
        </button>
        {#if path !== folder}
          <button class="iconbtn remove" onclick={() => onremove(path)} title="Remove from list" aria-label={`Remove ${name(path)} from the list`}><Icon name="x" size={14} /></button>
        {/if}
      </div>
    {/each}
    <button class="row" onclick={onchoose} disabled={busy}><span class="muted">Choose folder…</span></button>
  </div>
</div>
