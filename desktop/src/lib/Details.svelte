<script lang="ts">
  import { onDestroy } from "svelte";
  import { outputs, toolLabel, type Chat } from "./chat";
  import type { FileDiff } from "./diff";
  import type { Changes } from "./engine";
  import Icon from "./Icon.svelte";
  import { name, where } from "./paths";

  type Stats = { tokens?: { input?: number; output?: number; cacheRead?: number }; contextUsage?: { tokens?: number | null; contextWindow?: number | null; percent?: number | null } };
  let {
    folder,
    home,
    model,
    thinking,
    stats,
    changes,
    diff,
    extensions,
    chat,
    onopenfile,
    onshowdiff,
    onabort,
  }: {
    folder: string;
    home: string;
    model: { provider: string; id: string } | null;
    thinking: string;
    stats: Stats | null;
    changes: Changes | null;
    diff: FileDiff[];
    extensions: { name: string; description: string }[];
    chat: Chat;
    onopenfile: (path: string) => void;
    onshowdiff: () => void;
    onabort: () => void;
  } = $props();

  let contextOpen = $state(false);
  let showAll = $state(false);

  const files = $derived(outputs(chat));
  const tools = $derived(chat.items.filter((i) => i.kind === "tool"));
  const running = $derived(tools.filter((t) => t.running));
  const added = $derived(diff.reduce((n, f) => n + f.added, 0));
  const removed = $derived(diff.reduce((n, f) => n + f.removed, 0));
  const percent = $derived(stats?.contextUsage?.percent ?? null);

  // Elapsed time for running tools; events carry no start time, so note when each was first seen.
  const seen = new Map<string, number>();
  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(timer));
  function elapsed(id: string) {
    if (!seen.has(id)) seen.set(id, Date.now());
    const s = Math.max(0, Math.round((now - seen.get(id)!) / 1000));
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
  }

  const count = (n: number | null | undefined) => (n == null ? "–" : n.toLocaleString());
  const base = (path: string) => path.split("/").filter(Boolean).pop() ?? path;
</script>

<aside class="right">
  <section>
    <h3>Environment</h3>
    <div class="item"><Icon name="monitor" /> <span class="grow">This computer</span></div>
    <div class="item" title={folder}><Icon name="folder" /> <span class="grow ellipsis">{name(folder)} <span class="muted">· {where(folder, home)}</span></span></div>
    {#if changes?.branch}<div class="item" title={changes.branch}><Icon name="git-branch" /> <span class="grow ellipsis">{changes.branch}</span></div>{/if}
    <div class="item" title={model ? `${model.provider}/${model.id}` : ""}>
      <Icon name="cpu" /> <span class="grow ellipsis">{model ? model.id : "No model chosen"}</span>
      {#if model && thinking !== "off"}<span class="muted small">{thinking}</span>{/if}
    </div>
  </section>

  <section>
    <button class="item head" onclick={() => (contextOpen = !contextOpen)} aria-expanded={contextOpen}>
      <span class="grow">Context <Icon name={contextOpen ? "chevron-down" : "chevron-right"} size={12} /></span>
      {#if percent != null}
        <span class="muted small">{Math.round(percent)}%</span>
        <span class="meter"><span style:width={`${Math.min(100, percent)}%`}></span></span>
      {/if}
    </button>
    {#if contextOpen}
      <dl class="small">
        <dt>In context</dt><dd>{count(stats?.contextUsage?.tokens)} / {count(stats?.contextUsage?.contextWindow)}</dd>
        <dt>Input tokens</dt><dd>{count(stats?.tokens?.input)}</dd>
        <dt>Output tokens</dt><dd>{count(stats?.tokens?.output)}</dd>
        <dt>Cached reads</dt><dd>{count(stats?.tokens?.cacheRead)}</dd>
      </dl>
    {/if}
  </section>

  {#if extensions.length}
    <section>
      <h3>Extensions</h3>
      {#each extensions as e}
        <div class="item" title={e.description}><Icon name="package" /> <span class="grow ellipsis">{e.name}</span></div>
      {/each}
    </section>
  {/if}

  {#if files.length}
    <section>
      <h3>Outputs</h3>
      {#each showAll ? files : files.slice(0, 5) as path}
        <button class="item" onclick={() => onopenfile(path)} title={`Open ${path}`}><Icon name="file-text" /> <span class="grow ellipsis">{base(path)}</span></button>
      {/each}
      {#if files.length > 5}
        <button class="item muted" onclick={() => (showAll = !showAll)}>{showAll ? "Show less" : "Show more"}</button>
      {/if}
    </section>
  {/if}

  <section>
    <h3>Changes</h3>
    {#if !changes}
      <div class="item muted small">Not a git repository</div>
    {:else if diff.length}
      <button class="item" onclick={onshowdiff} title={changes.base ? `Changes since this branch left ${changes.base}` : "Uncommitted changes"}>
        <Icon name="git-commit" /> <span class="grow ellipsis">{name(folder)}</span>
        <span class="added small">+{added.toLocaleString()}</span>
        <span class="removed small">−{removed.toLocaleString()}</span>
      </button>
    {:else}
      <div class="item muted small">No changes on this branch</div>
    {/if}
  </section>

  {#if tools.length}
    <section>
      <h3>Background tasks</h3>
      {#each running as t (t.id)}
        <div class="item" title={toolLabel(t.name, t.args)}>
          <Icon name="terminal" /> <span class="grow ellipsis">{toolLabel(t.name, t.args)}</span>
          <span class="muted small">{elapsed(t.id)}</span>
          <button class="iconbtn" onclick={onabort} title="Stop the run (Esc)" aria-label="Stop the run"><Icon name="square" size={12} /></button>
        </div>
      {/each}
      <div class="item muted small"><Icon name="check" /> {tools.length - running.length} finished</div>
    </section>
  {/if}
</aside>
