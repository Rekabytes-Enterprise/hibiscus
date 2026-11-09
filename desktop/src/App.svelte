<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { apply, empty, fromMessages, type Chat } from "./lib/chat";
  import { gitChanges, listSessions, onEvent, openFile, openProject, post, request, type Changes, type SavedSession } from "./lib/engine";
  import ChatView from "./lib/ChatView.svelte";
  import Details from "./lib/Details.svelte";
  import DiffPane from "./lib/DiffPane.svelte";
  import { parseDiff } from "./lib/diff";
  import Composer from "./lib/Composer.svelte";
  import Icon from "./lib/Icon.svelte";
  import Menu from "./lib/Menu.svelte";
  import { name } from "./lib/paths";
  import Models from "./lib/Models.svelte";
  import NewSession from "./lib/NewSession.svelte";
  import Settings from "./lib/Settings.svelte";
  import Sidebar from "./lib/Sidebar.svelte";

  const RECENT = "hibiscus.recentFolders";

  let home = $state("");
  let folder = $state<string | null>(null);
  let recent = $state<string[]>(JSON.parse(localStorage.getItem(RECENT) ?? "[]"));
  // Saved sessions per recent folder, for the sidebar.
  let saved = $state<Record<string, SavedSession[]>>({});
  let chat = $state<Chat>(empty());
  let sessionFile = $state<string | null>(null);
  let model = $state<{ provider: string; id: string } | null>(null);
  let hasAccess = $state(true);
  let thinking = $state("off");
  let levels = $state<string[]>([]);
  let stats = $state<any>(null);
  let changes = $state<Changes | null>(null);
  // The right side shows session details, or the branch diff in their place.
  let side = $state<"details" | "diff">("details");
  let wide = $state(false);
  let extensions = $state<{ name: string; description: string }[]>([]);
  let panelOpen = $state(true);
  let dialog = $state<"models" | null>(null);
  let page = $state<"chat" | "settings">("chat");
  let error = $state<string | null>(null);
  let opening = $state(false);

  const busy = $derived(chat.running || opening);
  const diff = $derived(changes ? parseDiff(changes.diff) : []);
  const context = $derived(stats?.contextUsage?.percent ?? null);
  // The session panel only has something to say once the session has started.
  const active = $derived(page === "chat" && !!folder && (chat.items.length > 0 || chat.running));
  const title = $derived(
    (folder && saved[folder]?.find((s) => s.path === sessionFile)?.title) || "New session",
  );

  // Like a new chat: start in the last folder, or home the first time.
  onMount(async () => {
    home = await homeDir();
    if (recent[0]) await open(recent[0]);
    if (!folder) await open(home);
    for (const path of recent) if (!saved[path]) void loadSessions(path);
  });

  onMount(() =>
    onEvent((event) => {
      chat = apply(chat, event);
      if (event.type === "agent_settled") void refresh();
    }),
  );

  const message = (e: unknown) => String(e instanceof Error ? e.message : e);

  function notice(text: string) {
    chat = { ...chat, items: [...chat.items, { kind: "notice", text, error: true }] };
  }

  async function loadSessions(path: string) {
    try {
      saved[path] = await listSessions(path);
    } catch {
      // Moved or deleted: it can no longer be opened, so drop it from the sidebar.
      if (path === folder) return;
      recent = recent.filter((p) => p !== path);
      localStorage.setItem(RECENT, JSON.stringify(recent));
    }
  }

  // The branch the diff pane compares with, per folder; unset means the nearest base.
  const baseKey = (f: string) => `hibiscus.diffBase:${f}`;
  const pickedBase = (f: string) => localStorage.getItem(baseKey(f));
  async function pickBase(base: string) {
    if (!folder) return;
    localStorage.setItem(baseKey(folder), base);
    changes = await gitChanges(folder, base).catch(() => null);
  }

  async function refresh() {
    if (!folder) return;
    try {
      const state = await request({ type: "get_state" });
      model = state.model;
      hasAccess = state.hasAccess;
      thinking = state.thinkingLevel;
      sessionFile = state.sessionFile;
      extensions = state.extensionCommands ?? [];
      // Both fail while no model is selected; that is not an error to show.
      levels = await request({ type: "get_available_thinking_levels" }).then((r) => r.levels ?? [], () => []);
      stats = await request({ type: "get_session_stats" }).then((r) => r, () => null);
      changes = await gitChanges(folder, pickedBase(folder)).catch(() => null);
      await loadSessions(folder);
    } catch (e) {
      notice(message(e));
    }
  }

  /** Opens `path`: a new session, or a saved `session`. */
  async function open(path: string, session?: string) {
    opening = true;
    side = "details";
    wide = false;
    error = null;
    try {
      await openProject(path, false, session);
      folder = path;
      recent = [path, ...recent.filter((p) => p !== path)].slice(0, 8);
      localStorage.setItem(RECENT, JSON.stringify(recent));
      chat = session ? fromMessages((await request({ type: "get_messages" })).messages ?? []) : empty();
      await refresh();
    } catch (e) {
      error = message(e);
    } finally {
      opening = false;
    }
  }

  /** Asks for a folder and starts a new session there. */
  async function choose() {
    const path = await pickFolder({ directory: true, multiple: false, defaultPath: folder ?? undefined });
    if (typeof path === "string") await open(path);
  }

  async function newSession() {
    page = "chat";
    side = "details";
    wide = false;
    if (!folder) return open(home);
    try {
      await request({ type: "new_session" });
      chat = empty();
      await refresh();
    } catch (e) {
      notice(message(e));
    }
  }

  async function send(text: string, images: { mimeType: string; data: string }[], steer: boolean) {
    const command: any = { type: "prompt", message: text, images };
    if (chat.running) command.streamingBehavior = steer ? "steer" : "followUp";
    try {
      await post(command);
    } catch (e) {
      notice(message(e));
    }
  }

  async function setThinking(level: string) {
    try {
      await request({ type: "set_thinking_level", level });
    } catch (e) {
      notice(message(e));
    }
    await refresh();
  }

  /** Keys are read when an engine starts, as the TUI reconnects after /login. */
  async function signedIn() {
    if (folder) await open(folder, sessionFile ?? undefined);
  }

</script>

<div class="layout">
  <Sidebar
    {home}
    groups={recent.map((path) => ({ folder: path, sessions: saved[path] ?? [] }))}
    current={sessionFile}
    {busy}
    onnew={newSession}
    onresume={(path, session) => { page = "chat"; void open(path, session); }}
    onsettings={() => (page = "settings")}
  />
  <main>
    <div class="titlebar" data-tauri-drag-region>
      <span class="grow ellipsis" data-tauri-drag-region>{page === "settings" ? "Settings" : title}</span>
      {#if active}
        <button class="iconbtn" class:on={panelOpen} onclick={() => (panelOpen = !panelOpen)} title={panelOpen ? "Hide session details" : "Show session details"} aria-label="Session details" aria-pressed={panelOpen}><Icon name="panel" /></button>
      {/if}
    </div>
    <div class="body">
  {#if page === "settings"}
    <Settings onback={() => (page = "chat")} onsaved={signedIn} />
  {:else}
    <div class="center">
    {#if error}<div class="banner error">{error}</div>{/if}
    {#if !chat.items.length && !chat.running}
      <NewSession {folder} {recent} {busy} onopen={(p) => open(p)} onchoose={choose} onproviders={hasAccess ? undefined : () => (page = "settings")} />
    {:else}
      {#if !hasAccess}
        <div class="banner">No provider is signed in. <button class="link" onclick={() => (page = "settings")}>Add a provider in Settings</button></div>
      {/if}
      <ChatView {chat} {folder} onresend={(text) => { chat = { ...chat, rejected: chat.rejected.filter((t) => t !== text) }; send(text, [], false); }} />
    {/if}
    {#if folder}
      <div class="dock">
        <Composer running={chat.running} {chat} onsend={send} onabort={() => post({ type: "abort" }).catch(() => {})} />
        <div class="status">
          <Menu
            label="Working folder"
            title={`Working folder: ${folder}`}
            value={folder}
            disabled={busy}
            options={[...[folder, ...recent.filter((p) => p !== folder)].map((p) => ({ value: p, label: name(p), hint: p })), { value: "", label: "Choose folder…" }]}
            onpick={(p) => (p === "" ? choose() : p !== folder && open(p))}
          >
            {#snippet trigger()}<Icon name="folder" size={14} /><span>{name(folder ?? "")}</span>{/snippet}
          </Menu>
          <span class="grow"></span>
          {#if context != null}<span class="muted">{Math.round(context)}% context</span>{/if}
          <button class="plain picker" onclick={() => (dialog = "models")} disabled={chat.running}>
            <Icon name="cpu" size={14} /> {model ? model.id : "Choose a model"}
          </button>
          {#if levels.length > 1}
            <Menu
              label="Thinking level"
              title="Thinking level"
              value={thinking}
              disabled={chat.running}
              options={levels.map((l) => ({ value: l, label: l }))}
              onpick={(l) => l !== thinking && setThinking(l)}
            >
              {#snippet trigger()}<span>{thinking}</span>{/snippet}
            </Menu>
          {/if}
        </div>
      </div>
    {/if}
    </div>
  {/if}
  {#if active && panelOpen && side === "diff"}
    <DiffPane
      base={changes?.base ?? null}
      bases={changes?.bases ?? []}
      branch={changes?.branch ?? null}
      files={diff}
      truncated={changes?.truncated ?? false}
      {wide}
      onopenfile={(path) => openFile(folder!, path).catch((e) => notice(message(e)))}
      onbase={pickBase}
      onwide={() => (wide = !wide)}
      onclose={() => { side = "details"; wide = false; }}
    />
  {:else if active && panelOpen}
    <Details
      folder={folder!}
      {home}
      {model}
      {thinking}
      {stats}
      {changes}
      {diff}
      {extensions}
      {chat}
      onopenfile={(path) => openFile(folder!, path).catch((e) => notice(message(e)))}
      onshowdiff={() => { side = "diff"; void refresh(); }}
      onabort={() => post({ type: "abort" }).catch(() => {})}
    />
  {/if}
    </div>
  </main>
</div>

{#if dialog === "models"}
  <Models current={model} onclose={() => (dialog = null)} onchosen={async () => { dialog = null; await refresh(); }} />
{/if}
