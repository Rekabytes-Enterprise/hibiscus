<script lang="ts">
  import type { Snippet } from "svelte";
  import type { RekaAccount, SavedSession } from "./engine";
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";
  import PixelAvatar from "./PixelAvatar.svelte";
  import { name, where } from "./paths";

  let {
    home,
    groups,
    current,
    busy,
    working,
    onnew,
    onresume,
    onrename,
    ondelete,
    onaddfolder,
    oncanvas,
    oncollapse,
    canvas,
    onremovefolder,
    onmovefolder,
    onsettings,
    onaccount,
    account,
    onsignin,
    onsignout,
    update,
  }: {
    home: string;
    groups: { folder: string; sessions: SavedSession[] }[];
    current: string | null;
    /** A session is opening; nothing else can open until it has. */
    busy: boolean;
    /** Sessions running a turn, shown or in the background. */
    working: string[];
    onnew: () => void;
    onresume: (folder: string, session: string) => void;
    /** An empty name restores the automatic title. */
    onrename: (folder: string, session: string, name: string) => void;
    ondelete: (folder: string, session: string) => void;
    onaddfolder: () => void;
    /** Opens the canvas page, which `canvas` marks as showing. */
    oncanvas: () => void;
    /** Hides the sidebar; the title bar brings it back. */
    oncollapse: () => void;
    canvas: boolean;
    /** Takes the folder out of the list; nothing on disk changes. */
    onremovefolder: (folder: string) => void;
    /** Moves `from` into `to`'s place in the list. */
    onmovefolder: (from: string, to: string) => void;
    onsettings: () => void;
    /** Opens the Account page in Settings. */
    onaccount: () => void;
    /** The signed-in Reka Bytes account; without one the footer shows the computer's user. */
    account: RekaAccount | null;
    onsignin: () => void;
    onsignout: () => void;
    /** The update row, shown above the profile when a new version is out. */
    update?: Snippet;
  } = $props();

  // Signed out, the footer shows the computer's user.
  const user = $derived(account?.name || account?.email || name(home) || "");
  let profileOpen = $state(false);
  let profile: HTMLElement;

  function pick(action: () => void) {
    profileOpen = false;
    action();
  }

  function outside(e: PointerEvent) {
    if (profileOpen && !profile.contains(e.target as Node)) profileOpen = false;
  }
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

  // Dragging a folder header reorders the folders. Pointer events, not HTML drag
  // and drop, which the window's file drop takes over on Windows.
  let dragging = $state<string | null>(null);
  // Where the dragged folder is in the list: it lands above folders before it, below ones after.
  const from = $derived(groups.findIndex((g) => g.folder === dragging));
  let over = $state<string | null>(null);
  // The click that ends a drag doesn't fold the group.
  let dragged = false;
  function grab(e: PointerEvent, folder: string) {
    if (e.button !== 0) return;
    const y = e.clientY;
    const move = (m: PointerEvent) => {
      if (!dragging && Math.abs(m.clientY - y) < 4) return;
      dragging = folder;
      const head = document.elementFromPoint(m.clientX, m.clientY)?.closest<HTMLElement>("[data-folder]");
      over = head?.dataset.folder ?? null;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (dragging && over && over !== folder) onmovefolder(folder, over);
      dragged = !!dragging;
      setTimeout(() => (dragged = false));
      dragging = over = null;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** The folder actions; Move up and Move down do what dragging does, from the keyboard. */
  function folderActions(i: number) {
    return [
      ...(i > 0 ? [{ value: "up", label: "Move up" }] : []),
      ...(i < groups.length - 1 ? [{ value: "down", label: "Move down" }] : []),
      { value: "remove", label: "Remove from sidebar", hint: "Keeps the folder and its sessions on disk" },
    ];
  }

  function folderAction(action: string, i: number) {
    const folder = groups[i].folder;
    if (action === "up") onmovefolder(folder, groups[i - 1].folder);
    else if (action === "down") onmovefolder(folder, groups[i + 1].folder);
    else onremovefolder(folder);
  }

  const label = (path: string) => `${name(path)} · ${where(path, home)}`;
</script>

<svelte:window onpointerdown={outside} onkeydown={(e) => { if (profileOpen && e.key === "Escape") profileOpen = false; }} />

<aside class="left">
  <div class="traffic" data-tauri-drag-region>
    <button class="iconbtn" onclick={oncollapse} title="Hide sidebar" aria-label="Hide sidebar"><Icon name="sidebar" /></button>
  </div>
  <button class="nav strong" onclick={onnew} disabled={busy}><Icon name="plus" /> New session</button>
  <button class="nav" onclick={onaddfolder}><Icon name="folder-plus" /> Add folder…</button>
  <button class="nav" class:active={canvas} onclick={oncanvas} aria-pressed={canvas}><Icon name="layout" /> Canvas</button>
  <div class="groups">
    {#each groups as g, i (g.folder)}
      {@const target = !!dragging && over === g.folder && dragging !== g.folder}
      <div class="folder" data-folder={g.folder} class:dragging={dragging === g.folder} class:above={target && from > i} class:below={target && from < i}>
      <div class="grouphead">
        <button class="group muted small" onpointerdown={(e) => grab(e, g.folder)} onclick={() => { if (!dragged) closed[g.folder] = !closed[g.folder]; }} title={`${g.folder}\nDrag to reorder`} aria-expanded={!closed[g.folder]}>
          {label(g.folder)} <Icon name={closed[g.folder] ? "chevron-right" : "chevron-down"} size={12} />
        </button>
        <Menu
          actions
          label="Folder actions"
          title="Folder actions"
          options={folderActions(i)}
          onpick={(action) => folderAction(action, i)}
        >
          {#snippet trigger()}<Icon name="more-horizontal" size={14} />{/snippet}
        </Menu>
      </div>
      {#if !closed[g.folder]}
        <ul class="list">
          {#each g.sessions as s (s.path)}
            <li class:active={s.path === current}>
              {#if editing === s.path}
                <input class="rename" value={s.title} aria-label="Session name" onkeydown={(e) => rename(e, g.folder, s.path)} onblur={() => (editing = null)} {@attach focus} />
              {:else}
                <button class="row" class:active={s.path === current} onclick={() => onresume(g.folder, s.path)} disabled={busy || s.path === current} title={s.title}>
                  <span class="title">{s.title || "Untitled"}</span>
                  {#if working.includes(s.path)}<span class="spinner" role="status" aria-label="Working"></span>{/if}
                </button>
                <Menu
                  actions
                  label="Session actions"
                  title={working.includes(s.path) ? "Wait for this session to finish" : "Session actions"}
                  disabled={busy || working.includes(s.path)}
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
      </div>
    {/each}
  </div>
  {@render update?.()}
  <footer>
    <div class="menu profile grow" bind:this={profile}>
      <button class="profilebtn" aria-haspopup="menu" aria-expanded={profileOpen} onclick={() => (profileOpen = !profileOpen)}>
        {#if account}
          <span class="avatar pixel"><PixelAvatar id={account.avatar} size={20} /></span>
        {:else}
          <span class="avatar">{user.slice(0, 1).toUpperCase()}</span>
        {/if}
        <span class="username">{user}</span>
        <Icon name="chevron-down" size={12} />
      </button>
      {#if profileOpen}
        <div class="popover" role="menu" aria-label="Account">
          {#if account}
            <div class="who">
              <PixelAvatar id={account.avatar} size={32} />
              <div class="grow">
                <div class="ellipsis">{account.name || "Reka Bytes account"}</div>
                {#if account.email}<div class="small muted ellipsis">{account.email}</div>{/if}
              </div>
            </div>
            <button role="menuitem" onclick={() => pick(onaccount)}><Icon name="user" size={14} /> Manage account</button>
          {:else}
            <button role="menuitem" onclick={() => pick(onsignin)}><Icon name="log-in" size={14} /> Sign in with Reka Bytes</button>
          {/if}
          <button role="menuitem" onclick={() => pick(onsettings)}><Icon name="settings" size={14} /> Settings</button>
          {#if account}
            <button role="menuitem" onclick={() => pick(onsignout)}><Icon name="log-out" size={14} /> Sign out</button>
          {/if}
        </div>
      {/if}
    </div>
    <button class="iconbtn" onclick={onsettings} title="Settings" aria-label="Settings"><Icon name="settings" /></button>
  </footer>
</aside>
