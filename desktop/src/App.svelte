<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { ask, open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { onMount, tick } from "svelte";
  import { apply, ChatCache, empty, fromMessages, newer, type Chat } from "./lib/chat";
  import { closeProject, deleteSession, gitChanges, listSessions, onEvent, openFile, openProject, post, renameSession, request, type Changes, type SavedSession } from "./lib/engine";
  import BrowserPane from "./lib/BrowserPane.svelte";
  import { calls, POINTING } from "./lib/browser";
  import ChatView from "./lib/ChatView.svelte";
  import Details from "./lib/Details.svelte";
  import DiffPane from "./lib/DiffPane.svelte";
  import { parseDiff } from "./lib/diff";
  import Composer from "./lib/Composer.svelte";
  import Icon from "./lib/Icon.svelte";
  import Menu from "./lib/Menu.svelte";
  import { added, opened, removed } from "./lib/folders";
  import { name } from "./lib/paths";
  import Models from "./lib/Models.svelte";
  import NewSession from "./lib/NewSession.svelte";
  import Settings from "./lib/Settings.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import UpdateBanner from "./lib/UpdateBanner.svelte";

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
  // The right side shows session details, or the branch diff or browser in their place.
  let side = $state<"details" | "diff" | "browser">("details");
  // The agent-browser session Hibi used last, the element it acts on, and a
  // counter bumped after each of its agent-browser calls.
  let browserSession = $state<string | null>(null);
  let pointer = $state<{ target: string; seq: number } | null>(null);
  let poke = $state(0);
  const browserCalls = new Set<string>();
  let wide = $state(false);
  let extensions = $state<{ name: string; description: string }[]>([]);
  let panelOpen = $state(true);
  let dialog = $state<"models" | null>(null);
  let page = $state<"chat" | "settings">("chat");
  let error = $state<string | null>(null);
  let opening = $state(false);
  // Conversations opened this run, so going back to one shows it at once.
  const chats = new ChatCache();

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
      if (event.type === "tool_execution_start" && event.toolName === "bash") browsing(event.toolCallId, event.args?.command);
      if (event.type === "tool_execution_end" && browserCalls.delete(event.toolCallId)) poke++;
    }),
  );

  /** Opens the browser pane the first time Hibi uses agent-browser, and points at what it acts on. */
  function browsing(id: string, command: unknown) {
    const found = calls(String(command ?? ""));
    if (!found.length) return;
    browserCalls.add(id);
    if (side !== "browser") {
      side = "browser";
      panelOpen = true;
    }
    for (const c of found) {
      browserSession = c.session;
      if (POINTING.has(c.verb) && c.target) pointer = { target: c.target, seq: (pointer?.seq ?? 0) + 1 };
    }
  }

  const message = (e: unknown) => String(e instanceof Error ? e.message : e);

  /** Logs how long each step of `what` took since it began, to the dev console (#145). */
  function timer(what: string) {
    const start = performance.now();
    return (step: string) => console.debug(`[timing] ${what}: ${step} after ${Math.round(performance.now() - start)} ms`);
  }

  /** Logs `step` once the browser has drawn the current state. */
  const painted = (t: (step: string) => void, step: string) => tick().then(() => requestAnimationFrame(() => t(step)));

  /** Keeps the conversation on screen for a later return to its session. */
  function remember() {
    if (sessionFile && chat.items.length && !chat.running) chats.keep(sessionFile, chat);
  }

  function notice(text: string) {
    chat = { ...chat, items: [...chat.items, { kind: "notice", text, error: true }] };
  }

  function saveRecent(next: string[]) {
    recent = next;
    localStorage.setItem(RECENT, JSON.stringify(recent));
  }

  /** Adds a folder to the list without starting a session in it. */
  async function addFolder() {
    const path = await pickFolder({ directory: true, multiple: false, defaultPath: folder ?? undefined });
    if (typeof path !== "string") return;
    saveRecent(added(recent, path));
    void loadSessions(path);
  }

  /** Takes a folder out of the list only; its files and saved sessions stay. */
  function removeFolder(path: string) {
    saveRecent(removed(recent, path));
    delete saved[path];
  }

  async function loadSessions(path: string) {
    try {
      saved[path] = await listSessions(path);
    } catch {
      // Moved or deleted: it can no longer be opened, so drop it from the sidebar.
      if (path === folder) return;
      saveRecent(removed(recent, path));
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

  /**
   * Reloads the session details and stats, and with `folderData` the folder's
   * changes and session list, all at once. Switching sessions within a folder
   * changes neither of those, so it skips them.
   */
  async function refresh(folderData = true) {
    if (!folder) return;
    const path = folder;
    const t = timer("refresh");
    // None of these depends on another; each fills in its part when it arrives.
    await Promise.all([
      request({ type: "get_state" }).then((state) => {
        model = state.model;
        hasAccess = state.hasAccess;
        thinking = state.thinkingLevel;
        sessionFile = state.sessionFile;
        extensions = state.extensionCommands ?? [];
        t("get_state");
      }, (e) => notice(message(e))),
      // Both fail while no model is selected; that is not an error to show.
      request({ type: "get_available_thinking_levels" }).then((r) => r.levels ?? [], () => []).then((l) => { levels = l; t("thinking levels"); }),
      request({ type: "get_session_stats" }).then((r) => r, () => null).then((r) => { stats = r; t("session stats"); }),
      folderData && gitChanges(path, pickedBase(path)).catch(() => null).then((c) => { changes = c; t("git changes"); }),
      folderData && loadSessions(path).then(() => t("session list")),
    ]);
  }

  /**
   * Opens `path`: a new session, or a saved `session`. In the folder already open,
   * the running engine switches sessions; `restart` forces a new engine instead.
   */
  async function open(path: string, session?: string, restart = false) {
    opening = true;
    side = "details";
    wide = false;
    error = null;
    const t = timer(`open ${session ? "session" : "folder"}`);
    const inPlace = path === folder && !!session && !restart;
    const before = { chat, sessionFile };
    remember();
    // A session opened before shows at once; the engine's copy replaces it below if the file changed.
    const cached = session ? chats.get(session) : undefined;
    if (cached) {
      sessionFile = session!;
      chat = cached;
      painted(t, "cached conversation painted");
    }
    try {
      if (inPlace) {
        // No restart, so no model-list downloads either.
        await request({ type: "switch_session", sessionPath: session });
        t("switch_session");
      } else {
        await openProject(path, false, session);
        t("engine start");
      }
      folder = path;
      saveRecent(opened(recent, path));
      const fresh = session ? fromMessages((await request({ type: "get_messages" })).messages ?? []) : empty();
      // Set together, so the chat view is rebuilt once, with the new conversation.
      if (session) sessionFile = session;
      chat = cached ? newer(cached, fresh) : fresh;
      t("conversation loaded");
      painted(t, "conversation painted");
      opening = false;
      // Details fill in after the conversation is on screen.
      await refresh(!inPlace);
      t("details loaded");
      painted(t, "details painted");
    } catch (e) {
      error = message(e);
      ({ chat, sessionFile } = before);
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
    // A folder removed from the list comes back once a new session starts in it.
    saveRecent(opened(recent, folder));
    remember();
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

  /** Names a session; the open one through the engine, so its next save keeps the name. */
  async function rename(path: string, session: string, name: string) {
    try {
      if (path === folder && session === sessionFile) await request({ type: "set_session_name", name });
      else await renameSession(session, name);
    } catch (e) {
      notice(message(e));
    }
    await loadSessions(path);
  }

  async function remove(path: string, session: string) {
    if (!(await ask("Delete this session? This can't be undone.", { title: "Delete session", kind: "warning", okLabel: "Delete" }))) return;
    try {
      // The open session is replaced first, so the engine never saves it again.
      if (path === folder && session === sessionFile) {
        await newSession();
        if (sessionFile === session) return;
      }
      await deleteSession(session);
      chats.drop(session);
    } catch (e) {
      notice(message(e));
    }
    await loadSessions(path);
  }

  /** Keys are read when an engine starts, as the TUI reconnects after /login. */
  async function signedIn() {
    if (folder) await open(folder, sessionFile ?? undefined, true);
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
    onrename={rename}
    ondelete={remove}
    onaddfolder={addFolder}
    onremovefolder={removeFolder}
    onsettings={() => (page = "settings")}
  />
  <main>
    <div class="titlebar" data-tauri-drag-region>
      <span class="grow ellipsis" data-tauri-drag-region>{page === "settings" ? "Settings" : title}</span>
      {#if active}
        <button class="iconbtn" class:on={panelOpen && side === "browser"} onclick={() => { side = side === "browser" ? "details" : "browser"; panelOpen = true; wide = false; }} title={side === "browser" ? "Hide browser" : "Show browser"} aria-label="Browser" aria-pressed={panelOpen && side === "browser"}><Icon name="globe" /></button>
        <button class="iconbtn" class:on={panelOpen} onclick={() => (panelOpen = !panelOpen)} title={panelOpen ? "Hide session details" : "Show session details"} aria-label="Session details" aria-pressed={panelOpen}><Icon name="panel" /></button>
      {/if}
    </div>
    <UpdateBanner {busy} onrestart={() => closeProject().catch(() => {})} />
    <div class="body">
  {#if page === "settings"}
    <Settings onback={() => (page = "chat")} onsaved={signedIn} />
  {:else}
    <div class="center">
    {#if error}<div class="banner error">{error}</div>{/if}
    {#if !chat.items.length && !chat.running}
      <NewSession {folder} {recent} {busy} onopen={(p) => open(p)} onchoose={choose} onremove={removeFolder} onproviders={hasAccess ? undefined : () => (page = "settings")} />
    {:else}
      {#if !hasAccess}
        <div class="banner">No provider is signed in. <button class="link" onclick={() => (page = "settings")}>Add a provider in Settings</button></div>
      {/if}
      {#key sessionFile}<ChatView {chat} {folder} onresend={(text) => { chat = { ...chat, rejected: chat.rejected.filter((t) => t !== text) }; send(text, [], false); }} />{/key}
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
  {#if active && panelOpen && side === "browser"}
    <BrowserPane
      session={browserSession}
      {pointer}
      {poke}
      {wide}
      onwide={() => (wide = !wide)}
      onclose={() => { side = "details"; wide = false; }}
    />
  {:else if active && panelOpen && side === "diff"}
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
