<script lang="ts">
  import type { SavedSession } from "./engine";
  import Icon from "./Icon.svelte";
  import { name, where } from "./paths";

  let {
    home,
    groups,
    current,
    busy,
    onnew,
    onresume,
    onsettings,
  }: {
    home: string;
    groups: { folder: string; sessions: SavedSession[] }[];
    current: string | null;
    busy: boolean;
    onnew: () => void;
    onresume: (folder: string, session: string) => void;
    onsettings: () => void;
  } = $props();

  // Hibiscus has no account; the footer shows the computer's user.
  const user = $derived(home.split("/").filter(Boolean).pop() ?? "");
  let closed = $state<Record<string, boolean>>({});

  const label = (path: string) => `${name(path)} · ${where(path, home)}`;
</script>

<aside class="left">
  <div class="traffic" data-tauri-drag-region></div>
  <button class="nav strong" onclick={onnew} disabled={busy}><Icon name="plus" /> New session</button>
  <div class="groups">
    {#each groups as g (g.folder)}
      <button class="group muted small" onclick={() => (closed[g.folder] = !closed[g.folder])} title={g.folder} aria-expanded={!closed[g.folder]}>
        {label(g.folder)} <Icon name={closed[g.folder] ? "chevron-right" : "chevron-down"} size={12} />
      </button>
      {#if !closed[g.folder]}
        <ul class="list">
          {#each g.sessions as s (s.path)}
            <li>
              <button class:active={s.path === current} onclick={() => onresume(g.folder, s.path)} disabled={busy || s.path === current} title={s.title}>
                <span class="title">{s.title || "Untitled"}</span>
              </button>
            </li>
          {:else}
            <li class="muted small empty">No sessions yet</li>
          {/each}
        </ul>
      {/if}
    {/each}
  </div>
  <footer>
    <span class="avatar">{user.slice(0, 1).toUpperCase()}</span>
    <span class="grow username">{user}</span>
    <button class="iconbtn" onclick={onsettings} title="Settings" aria-label="Settings"><Icon name="settings" /></button>
  </footer>
</aside>
