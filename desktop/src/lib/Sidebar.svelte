<script lang="ts">
  import type { SavedSession } from "./engine";
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";
  import { name, where } from "./paths";

  let {
    home,
    groups,
    current,
    busy,
    onnew,
    onresume,
    onrename,
    ondelete,
    onsettings,
  }: {
    home: string;
    groups: { folder: string; sessions: SavedSession[] }[];
    current: string | null;
    busy: boolean;
    onnew: () => void;
    onresume: (folder: string, session: string) => void;
    /** An empty name restores the automatic title. */
    onrename: (folder: string, session: string, name: string) => void;
    ondelete: (folder: string, session: string) => void;
    onsettings: () => void;
  } = $props();

  // Hibiscus has no account; the footer shows the computer's user.
  const user = $derived(home.split("/").filter(Boolean).pop() ?? "");
  let closed = $state<Record<string, boolean>>({});
  // The session whose title is being edited.
  let editing = $state<string | null>(null);

  function rename(e: KeyboardEvent, folder: string, session: string) {
    if (e.key === "Escape") editing = null;
    if (e.key !== "Enter") return;
    const name = (e.currentTarget as HTMLInputElement).value;
    editing = null;
    onrename(folder, session, name);
  }

  const focus = (input: HTMLInputElement) => {
    input.focus();
    input.select();
  };

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
            <li class:active={s.path === current}>
              {#if editing === s.path}
                <input class="rename" value={s.title} aria-label="Session name" onkeydown={(e) => rename(e, g.folder, s.path)} onblur={() => (editing = null)} {@attach focus} />
              {:else}
                <button class="row" class:active={s.path === current} onclick={() => onresume(g.folder, s.path)} disabled={busy || s.path === current} title={s.title}>
                  <span class="title">{s.title || "Untitled"}</span>
                </button>
                <Menu
                  actions
                  label="Session actions"
                  title="Session actions"
                  disabled={busy}
                  options={[{ value: "rename", label: "Rename" }, { value: "delete", label: "Delete" }]}
                  onpick={(action) => (action === "rename" ? (editing = s.path) : ondelete(g.folder, s.path))}
                >
                  {#snippet trigger()}<Icon name="more-horizontal" size={16} />{/snippet}
                </Menu>
              {/if}
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
