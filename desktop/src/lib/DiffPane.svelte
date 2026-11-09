<script lang="ts">
  import type { FileDiff } from "./diff";
  import Hunks from "./Hunks.svelte";
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";

  let {
    base,
    bases,
    branch,
    files,
    truncated,
    wide,
    onopenfile,
    onbase,
    onwide,
    onclose,
  }: {
    base: string | null;
    bases: string[];
    branch: string | null;
    files: FileDiff[];
    truncated: boolean;
    wide: boolean;
    onopenfile: (path: string) => void;
    /** The user picked another branch to compare with. */
    onbase: (base: string) => void;
    onwide: () => void;
    onclose: () => void;
  } = $props();

  // Big files start folded so opening the pane stays fast.
  const BIG = 1500;
  let folded = $state<Record<string, boolean>>({});
  const isFolded = (f: FileDiff) => folded[f.path] ?? f.hunks.reduce((n, h) => n + h.lines.length, 0) > BIG;
</script>

<aside class="diff" class:wide>
  <header>
    <!-- The menu sits outside the clipped branch name so its list can open. -->
    <span class="grow refs">
      {#if base}
        <Menu options={bases.map((b) => ({ value: b, label: b }))} value={base} label="Compare with" title="Branch to compare with" onpick={onbase}>
          {#snippet trigger()}<span class="muted">{base}</span>{/snippet}
        </Menu>
        <Icon name="arrow-right" size={12} />
      {/if}
      <span class="ellipsis">{branch ?? "Detached HEAD"}</span>
    </span>
    <button class="iconbtn" onclick={onwide} title={wide ? "Shrink" : "Expand"} aria-label={wide ? "Shrink" : "Expand"}><Icon name={wide ? "minimize" : "maximize"} size={14} /></button>
    <button class="iconbtn" onclick={onclose} title="Close" aria-label="Close changes"><Icon name="x" size={14} /></button>
  </header>
  <div class="files">
    {#if truncated}<div class="note">This diff is too large to show in full; later files are left out.</div>{/if}
    {#each files as f (f.path)}
      <section class="file">
        <div class="filehead">
          <button class="name" onclick={() => (folded[f.path] = !isFolded(f))} aria-expanded={!isFolded(f)}>
            <Icon name={isFolded(f) ? "chevron-right" : "chevron-down"} size={12} />
            <span class="ellipsis" title={f.path}>{f.path}</span>
            {#if f.status !== "modified"}<span class="muted small">{f.status}</span>{/if}
          </button>
          <span class="added small">+{f.added}</span>
          <span class="removed small">−{f.removed}</span>
          {#if f.status !== "deleted"}
            <button class="iconbtn" onclick={() => onopenfile(f.path)} title="Open file" aria-label={`Open ${f.path}`}><Icon name="arrow-up-right" size={14} /></button>
          {/if}
        </div>
        {#if !isFolded(f)}
          {#if f.binary}
            <div class="note">Binary file changed</div>
          {:else if !f.hunks.length}
            <div class="note">No line changes</div>
          {/if}
          <Hunks hunks={f.hunks} />
        {/if}
      </section>
    {:else}
      <div class="note">No changes on this branch.</div>
    {/each}
  </div>
</aside>
