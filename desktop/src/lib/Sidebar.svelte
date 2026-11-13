<script lang="ts">
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
    onnew,
    onresume,
    onrename,
    ondelete,
    onaddfolder,
    onremovefolder,
    onsettings,
    onaccount,
    account,
    onsignin,
    onsignout,
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
    onaddfolder: () => void;
    /** Takes the folder out of the list; nothing on disk changes. */
    onremovefolder: (folder: string) => void;
    onsettings: () => void;
    /** Opens the Account page in Settings. */
    onaccount: () => void;
    /** The signed-in Reka Bytes account; without one the footer shows the computer's user. */
    account: RekaAccount | null;
    onsignin: () => void;
    onsignout: () => void;
  } = $props();

  // Signed out, the footer shows the computer's user.
  const user = $derived(account?.name || account?.email || home.split("/").filter(Boolean).pop() || "");
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

  const label = (path: string) => `${name(path)} · ${where(path, home)}`;
</script>

<svelte:window onpointerdown={outside} onkeydown={(e) => { if (profileOpen && e.key === "Escape") profileOpen = false; }} />

<aside class="left">
  <div class="traffic" data-tauri-drag-region></div>
  <button class="nav strong" onclick={onnew} disabled={busy}><Icon name="plus" /> New session</button>
  <button class="nav" onclick={onaddfolder}><Icon name="folder-plus" /> Add folder…</button>
  <div class="groups">
    {#each groups as g (g.folder)}
      <div class="grouphead">
        <button class="group muted small" onclick={() => (closed[g.folder] = !closed[g.folder])} title={g.folder} aria-expanded={!closed[g.folder]}>
          {label(g.folder)} <Icon name={closed[g.folder] ? "chevron-right" : "chevron-down"} size={12} />
        </button>
        <Menu
          actions
          label="Folder actions"
          title="Folder actions"
          options={[{ value: "remove", label: "Remove from sidebar", hint: "Keeps the folder and its sessions on disk" }]}
          onpick={() => onremovefolder(g.folder)}
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
