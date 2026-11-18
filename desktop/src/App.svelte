<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { homeDir } from "@tauri-apps/api/path";
  import { ask, open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { onMount, tick, untrack } from "svelte";
  import { apply, ChatCache, empty, fromMessages, newer, type Chat } from "./lib/chat";
  import { canvasStyles, type Style, canvasLoad, canvasStore, readReference, saveReference, closeProject, deleteSession, gitChanges, listSessions, onEvent, openFile, openProject, post, rekaAccount, rekaProfile, rekaSignInError, renameSession, request, signOut, wslConnect, wslDistros, wslHome, type Distro, type RekaAccount, type Changes, type SavedSession } from "./lib/engine";
  import BrowserPane from "./lib/BrowserPane.svelte";
  import { POINTING } from "./lib/browser";
  import ChatView from "./lib/ChatView.svelte";
  import Details from "./lib/Details.svelte";
  import DiffPane from "./lib/DiffPane.svelte";
  import { parseDiff } from "./lib/diff";
  import Composer from "./lib/Composer.svelte";
  import Icon from "./lib/Icon.svelte";
  import Menu from "./lib/Menu.svelte";
  import { added, opened, removed } from "./lib/folders";
  import { name, wslDistro } from "./lib/paths";
  import Models from "./lib/Models.svelte";
  import NewSession from "./lib/NewSession.svelte";
  import Settings from "./lib/Settings.svelte";
  import Canvas from "./lib/Canvas.svelte";
  import * as prompts from "./lib/prompts";
  import { answers, board, busy as cardBusy, restore, serialize, styleOf, fillFrames, openFrames, openPreview, openQuestions, openScript, openStoryboard, outOfSync, removeCard, scriptOf, voiced, type Card, type Reference } from "./lib/board.svelte";
  import SignIn from "./lib/SignIn.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import UpdateBanner from "./lib/UpdateBanner.svelte";

  const RECENT = "hibiscus.recentFolders";
  // Set by "Continue with my own provider": no sign-in screen at launch (#185).
  const OWN_PROVIDER = "hibiscus.ownProvider";

  // The Reka Bytes sign-in screen: null until the stored account is checked.
  let gate = $state<boolean | null>(null);
  // The engine's error for a Rooxcode prompt without a session.
  let signInError = "";

  let home = $state("");
  /** The signed-in Reka Bytes account for the sidebar profile, or null. */
  let account = $state<RekaAccount | null>(null);
  /** The Settings page to open on. */
  let settingsStart = $state<"account" | "providers" | "connectors">("providers");
  let folder = $state<string | null>(null);

  // WSL 2 on Windows (#201): the distros found at startup, and the distro each
  // folder is connected to. Connecting is always the user's choice.
  const WSL = "hibiscus.wsl:";
  let distros = $state<Distro[]>([]);
  let wsl = $state<Record<string, string>>(
    Object.fromEntries(
      Object.keys(localStorage)
        .filter((k) => k.startsWith(WSL))
        .map((k) => [k.slice(WSL.length), localStorage.getItem(k) ?? ""]),
    ),
  );
  // A folder inside a distro always runs there (#206).
  const inside = $derived(folder ? wslDistro(folder) : undefined);
  const connected = $derived(folder ? (inside ?? wsl[folder]) : undefined);
  // The last WSL choice, so pickers start in the distro with no folder open.
  const LAST_WSL = "hibiscus.wslLast";

  async function connectWsl(distro: string | null) {
    if (!folder) return;
    const f = folder;
    try {
      await wslConnect(f, distro);
    } catch (e) {
      error = message(e);
      return;
    }
    if (distro) localStorage.setItem(LAST_WSL, distro);
    else localStorage.removeItem(LAST_WSL);
    if (distro) {
      localStorage.setItem(WSL + f, distro);
      wsl[f] = distro;
    } else {
      localStorage.removeItem(WSL + f);
      delete wsl[f];
    }
  }
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
  // The browser opens as its own pane at the far right, beside them.
  let browserOpen = $state(false);
  // The element the browser tool acts on, and a counter bumped after each
  // browser call. The pane opens by itself once per run.
  let pointer = $state<{ target: string; seq: number } | null>(null);
  let poke = $state(0);
  const browserCalls = new Set<string>();
  let browserShown = false;
  let browserWide = $state(false);
  let composer = $state<ReturnType<typeof Composer>>();
  let wide = $state(false);
  let extensions = $state<{ name: string; description: string }[]>([]);
  let panelOpen = $state(true);
  let dialog = $state<"models" | null>(null);
  let page = $state<"chat" | "settings" | "canvas">("chat");
  // The left sidebar collapses from the title bar; the choice is remembered.
  const SIDEBAR = "hibiscus.sidebarClosed";
  let sidebarOpen = $state(!localStorage.getItem(SIDEBAR));
  // Dragging the sidebar's edge sets its width, kept between launches.
  const SIDEBAR_WIDTH = "hibiscus.sidebarWidth";
  const clampWidth = (w: number) => Math.min(480, Math.max(200, Math.round(w)));
  let sidebarWidth = $state(clampWidth(Number(localStorage.getItem(SIDEBAR_WIDTH)) || 260));
  function resizeSidebar(e: PointerEvent) {
    e.preventDefault();
    document.body.classList.add("resizing");
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const start = e.clientX - sidebarWidth;
    const move = (m: PointerEvent) => (sidebarWidth = clampWidth(m.clientX - start));
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", () => {
      handle.removeEventListener("pointermove", move);
      document.body.classList.remove("resizing");
      localStorage.setItem(SIDEBAR_WIDTH, String(sidebarWidth));
    }, { once: true });
  }
  function toggleSidebar() {
    sidebarOpen = !sidebarOpen;
    if (sidebarOpen) localStorage.removeItem(SIDEBAR);
    else localStorage.setItem(SIDEBAR, "1");
  }
  let error = $state<string | null>(null);
  let opening = $state(false);
  // A session still loading: its row and title change on the click (#171).
  let pending = $state<string | null>(null);
  // Set once that load is slow enough to show placeholders.
  let loading = $state(false);
  // Conversations opened this run, so going back to one shows it at once.
  const chats = new ChatCache();

  const busy = $derived(chat.running || opening);
  const diff = $derived(changes ? parseDiff(changes.diff) : []);
  const context = $derived(stats?.contextUsage?.percent ?? null);
  // The session panel only has something to say once the session has started.
  const active = $derived(page === "chat" && !!folder && (chat.items.length > 0 || chat.running));
  // The browser is there before a session starts too, to open a page by hand.
  // Each session keeps its own canvas, saved with it. `boardSession` is the session the
  // cards on screen belong to; a canvas started before the first message joins the
  // session that message creates.
  let boardSession: string | null = null;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const json = serialize();
    const session = boardSession;
    if (!session) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void canvasStore(session, json).catch(() => {}), 600);
  });
  $effect(() => {
    const session = sessionFile;
    untrack(() => void switchBoard(session));
  });
  async function switchBoard(session: string | null) {
    if (session === boardSession) return;
    if (boardSession) {
      clearTimeout(saveTimer);
      await canvasStore(boardSession, serialize()).catch(() => {});
    }
    if (!boardSession && session && board.cards.length) {
      boardSession = session;
      return;
    }
    boardSession = session;
    restore(session ? await canvasLoad(session).catch(() => null) : null, readReference);
  }

  /** Video styles from ~/.hibiscus/skills, read when the app starts. */
  let styles = $state<Style[]>([]);
  /** The model error from the current canvas run, if its reply failed. */
  let runError: string | null = null;
  /** The current canvas run's last tool step, and whether a reply has followed it. */
  let lastStep: string | undefined;
  let afterTool: string | undefined;
  void canvasStyles().then((s) => (styles = s)).catch(() => {});

  // A session started from a canvas card: its title bar links back to the canvas.
  const fromCanvas = $derived(chat.items.some((i) => i.kind === "user" && i.text.startsWith("[Canvas ")));
  const showBrowser = $derived(page === "chat" && !!folder && browserOpen);
  const title = $derived(
    (folder && saved[folder]?.find((s) => s.path === (pending ?? sessionFile))?.title) || "New session",
  );

  onMount(async () => {
    signInError = await rekaSignInError().catch(() => "");
    account = await rekaAccount().catch(() => null);
    gate = !account && !localStorage.getItem(OWN_PROVIDER);
    // The saved profile shows at once; a name or avatar changed on the website follows.
    if (account) account = await rekaProfile().catch(() => account);
  });

  async function loadAccount() {
    account = await rekaProfile().catch(() => null);
  }

  async function rekaSignOut() {
    try {
      await signOut("rooxcode");
    } catch (e) {
      error = message(e);
      return;
    }
    account = null;
    showSignIn();
  }

  /** Shows the sign-in screen again, after signing out or for a Rooxcode prompt. */
  function showSignIn() {
    localStorage.removeItem(OWN_PROVIDER);
    void loadAccount();
    gate = true;
  }

  const needsSignIn = (text: unknown) => !!signInError && String(text ?? "").includes(signInError);

  // Like a new chat: start in the last folder, or home the first time.
  onMount(async () => {
    // Remembered WSL connections apply before any folder opens; detection runs
    // in the background and only decides whether the button shows.
    await Promise.all(Object.entries(wsl).map(([f, d]) => wslConnect(f, d).catch(() => {})));
    void wslDistros().then((d) => (distros = d)).catch(() => {});
    home = await homeDir();
    if (recent[0]) await open(recent[0]);
    if (!folder) await open(home);
    for (const path of recent) if (!saved[path]) void loadSessions(path);
  });

  onMount(() =>
    onEvent((event) => {
      chat = apply(chat, event);
      if (event.type === "agent_settled") void refresh();
      // A session revoked or expired on the gateway mid-run.
      if (event.type === "message_end" && needsSignIn(event.message?.errorMessage)) showSignIn();
      if (event.type === "agent_start") browserShown = false;
      if (event.type === "tool_execution_start" && event.toolName === "browser") browsing(event.toolCallId, event.args?.args);
      // The agent's questions open as a card with suggested answers.
      if (event.type === "tool_execution_start" && event.toolName === "ask_user") {
        openQuestions(event.args ?? {});
        page = "canvas";
      }
      // The storyboard and the video preview open on the canvas for the user to review.
      if (event.type === "tool_execution_start" && event.toolName === "storyboard") {
        openStoryboard(event.args ?? {});
        page = "canvas";
      }
      if (event.type === "tool_execution_start" && event.toolName === "video_preview") {
        openPreview(event.args ?? {});
        page = "canvas";
      }
      // A video's frames open on the canvas as the agent cuts them, joined to its card.
      if (event.type === "tool_execution_start" && event.toolName === "analyze_video") {
        openFrames(event.toolCallId, event.args?.path ?? "video");
        page = "canvas";
      }
      if (event.type === "tool_execution_end" && event.toolName === "analyze_video") fillFrames(event.toolCallId, event.result, !!event.isError);
      // What the agent is doing for the card it works for, shown live on that card.
      const working = board.cards.find((c) => c.id === board.active);
      // A run that ends right after a tool result, with no reply, stopped before finishing.
      if (event.type === "agent_start") afterTool = undefined;
      if (event.type === "tool_execution_start") lastStep = activity(event.toolName, event.args);
      if (event.type === "tool_execution_end") afterTool = lastStep;
      if (event.type === "message_end" && event.message?.role === "assistant" && !event.message?.errorMessage) afterTool = undefined;
      if (event.type === "tool_execution_end" && event.toolName === "video_preview" && !event.isError) void checkSync(event.result?.details);
      if (working) {
        if (event.type === "agent_start") working.activity = "Thinking";
        if (event.type === "tool_execution_start") working.activity = activity(event.toolName, event.args);
        if (event.type === "tool_execution_end") working.activity = "Thinking";
        if (event.type === "message_update") working.activity = working.activity === "Thinking" || working.activity === "Writing" ? "Writing" : working.activity;
      }
      // A failed model reply (a refused request, a budget limit) shows on the card, not just in the chat.
      if (event.type === "message_end" && event.message?.errorMessage && board.active !== undefined) runError = String(event.message.errorMessage);
      if (event.type === "agent_settled") {
        const sent = board.cards.find((c) => c.id === board.active);
        if (sent) {
          sent.stopped = runError ? undefined : afterTool;
          sent.status = runError ? `The model failed: ${runError}` : afterTool ? `Stopped before finishing, after: ${afterTool}.` : "Done. The agent's reply is in the chat.";
        }
        runError = null;
        afterTool = undefined;
        if (sent) {
          sent.activity = undefined;
          sent.started = undefined;
        }
        board.active = undefined;
      }
      // A video's voiceover script opens on the canvas for the user to review.
      if (event.type === "tool_execution_start" && event.toolName === "text_to_speech") {
        openScript(model, event.args ?? {}, { thinking, levels });
        page = "canvas";
      }
      // Hibi starting the dev server opens the pane, where its output shows.
      if (event.type === "tool_execution_start" && event.toolName === "dev_server") {
        browserCalls.add(event.toolCallId);
        if (event.args?.action === "start") browserOpen = true;
      }
      if (event.type === "tool_execution_end" && browserCalls.delete(event.toolCallId)) poke++;
    }),
  );

  /** Closing the pane closes the browser too, so it opens blank next time. */
  function closeBrowser() {
    browserOpen = false;
    browserWide = false;
    void invoke("browser", { args: ["close"] }).catch(() => {});
  }

  /** Shows the browser pane on the run's first browser call, and points at the element it acts on. */
  function browsing(id: string, args: unknown) {
    browserCalls.add(id);
    if (!browserShown) {
      browserShown = true;
      browserOpen = true;
    }
    const [verb, target] = Array.isArray(args) ? args.map(String) : [];
    if (POINTING.has(verb) && target && !target.startsWith("-")) pointer = { target, seq: (pointer?.seq ?? 0) + 1 };
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

  /** While connected to WSL, pickers open in the distro's home (#206). */
  async function pickStart() {
    const distro = connected ?? (folder ? null : localStorage.getItem(LAST_WSL));
    const start = distro ? await wslHome(distro).catch(() => null) : null;
    return start ?? folder ?? undefined;
  }

  /** Adds a folder to the list without starting a session in it. */
  async function addFolder() {
    const path = await pickFolder({ directory: true, multiple: false, defaultPath: await pickStart() });
    if (typeof path !== "string") return;
    saveRecent(added(recent, path));
    void loadSessions(path);
  }

  /** Takes a folder out of the list only; its files and saved sessions stay. */
  /** A folder's dev server stops when the folder closes. */
  const stopServer = (path: string) => void invoke("dev_server_stop", { folder: path }).catch(() => {});

  function removeFolder(path: string) {
    stopServer(path);
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
    if (folder && path !== folder) stopServer(folder);
    const before = { chat, sessionFile };
    remember();
    // A session opened before shows at once; the engine's copy replaces it below if the file changed.
    const cached = session ? chats.get(session) : undefined;
    let slow: ReturnType<typeof setTimeout> | undefined;
    const settle = () => {
      clearTimeout(slow);
      pending = null;
      loading = false;
    };
    if (cached) {
      sessionFile = session!;
      chat = cached;
      painted(t, "cached conversation painted");
    } else if (session) {
      pending = session;
      // Fast loads skip the placeholders, so the page doesn't flicker.
      slow = setTimeout(() => (loading = true), 150);
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
      const fresh = session ? await latest() : empty();
      // Set together, so the chat view is rebuilt once, with the new conversation.
      if (session) sessionFile = session;
      chat = cached ? newer(cached, fresh) : fresh;
      settle();
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
      settle();
      opening = false;
    }
  }

  // Messages per page of a saved session: the latest first, older ones on scroll (#171).
  const PAGE = 60;

  /** The latest page of the engine's open session. */
  async function latest(): Promise<Chat> {
    const page = await request({ type: "get_messages", limit: PAGE });
    return { ...fromMessages(page.messages ?? []), start: page.start ?? 0, edited: page.edited ?? [] };
  }

  /** Adds the page before the oldest loaded message, as the user scrolls up. */
  async function older() {
    const session = sessionFile;
    if (!chat.start) return;
    try {
      const page = await request({ type: "get_messages", limit: PAGE, before: chat.start });
      if (sessionFile !== session) return;
      chat = { ...chat, items: [...fromMessages(page.messages ?? []).items, ...chat.items], start: page.start ?? 0 };
    } catch (e) {
      notice(message(e));
    }
  }

  /** Asks for a folder and starts a new session there. */
  async function choose() {
    const path = await pickFolder({ directory: true, multiple: false, defaultPath: await pickStart() });
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
      // Refused before it was sent: the text goes back in the message box.
      if (needsSignIn(message(e))) {
        composer?.add(text, images);
        showSignIn();
      } else notice(message(e));
    }
  }

  /** A short label for a tool call, shown on the card while the agent works. */
  function activity(tool: string, args: any): string {
    const file = (p: unknown) => String(p ?? "").split(/[\\/]/).pop();
    switch (tool) {
      case "read": return `Reading ${file(args?.path)}`;
      case "grep": return "Searching the code";
      case "glob": return "Finding files";
      case "write": return `Writing ${file(args?.path)}`;
      case "edit": return `Editing ${file(args?.path)}`;
      case "bash": return /hyperframes\s+(check|lint)/.test(args?.command ?? "") ? "Checking the video" : /hyperframes\s+(snapshot)/.test(args?.command ?? "") ? "Taking snapshots" : /hyperframes\s+render/.test(args?.command ?? "") ? "Rendering" : /transcribe/.test(args?.command ?? "") ? "Transcribing the voice" : "Running a command";
      case "storyboard": return "Drawing the storyboard";
      case "text_to_speech": return "Sending the script";
      case "analyze_video": return "Analyzing the video";
      case "video_preview": return "Opening the preview";
      case "ask_user": return "Asking you a question";
      default: return `Using ${tool}`;
    }
  }

  /** An image reference as message attachment data. */
  async function attachment(url: string) {
    const blob = await (await fetch(url)).blob();
    const data = await new Promise<string>((done) => {
      const reader = new FileReader();
      reader.onload = () => done(String(reader.result));
      reader.readAsDataURL(blob);
    });
    return { mimeType: blob.type || "image/png", data: data.slice(data.indexOf(",") + 1) };
  }

  /**
   * Sends a canvas card to the agent in the open folder's session: its text, its
   * references (saved to disk so the agent can open them; images also attached),
   * and its model and reasoning level, which become the session's.
   */
  /** Saves a file held in the window (a blob or data URL) where the agent can read it. */
  const toDisk = async (name: string, url: string) => saveReference(name, new Uint8Array(await (await fetch(url)).arrayBuffer()));

  async function sendCard(card: Card, prompt?: string, extra: Reference[] = []) {
    if (!folder) {
      card.status = "Open a folder first. The agent works in it.";
      return;
    }
    // Locks the card's buttons at once, so a second press can't send it twice.
    if (cardBusy(card)) return;
    card.status = "Sending…";
    board.active = card.parent ?? card.id;
    const working = board.cards.find((c) => c.id === board.active);
    if (working) {
      working.started = Date.now();
      working.activity = "Starting";
      working.stopped = undefined;
    }
    try {
      const images: { mimeType: string; data: string }[] = [];
      const lines: string[] = [];
      for (const ref of [...card.refs, ...extra]) {
        if (!ref.path && ref.url && /^(blob|data):/.test(ref.url)) ref.path = await toDisk(ref.name, ref.url);
        // The card's own images go along as pictures; extra ones, such as frames, by path only.
        if (!ref.video && ref.url && card.refs.includes(ref)) images.push(await attachment(ref.url));
        if (ref.path) lines.push(`- ${ref.time ? `frame at ${ref.time}` : ref.video ? "video" : "image"}: ${ref.path}`);
      }
      if (card.model && (card.model.provider !== model?.provider || card.model.id !== model?.id)) {
        await request({ type: "set_model", provider: card.model.provider, modelId: card.model.id });
      }
      if (card.thinking && card.thinking !== thinking) await request({ type: "set_thinking_level", level: card.thinking });
      await refresh();
      const kind = { video: "Video", image: "Image", speech: "Text to speech", frames: "Frames", storyboard: "Storyboard", preview: "Video preview", question: "Questions" }[card.kind];
      // The video's style skill is named in every step, so the series stays consistent.
      const style = styleFor(card);
      const rule = style ? `\n\n${prompts.styleRule(style.name, style.path)}` : "";
      const text = `[Canvas ${kind} card]\n${(prompt ?? card.text).trim()}${rule}${lines.length ? `\n\nReferences:\n${lines.join("\n")}` : ""}`;
      // Cards the agent opens next join the video this work is for.
      board.active = card.parent ?? card.id;
      await send(text, images, false);
      card.status = "Sent. The agent is working on it in the chat.";
    } catch (e) {
      card.status = message(e);
      board.active = undefined;
    }
  }

  /** The frames the user left on, as style references for this video. */
  const framesOf = (video: Card) => board.cards.filter((c) => c.parent === video.id && c.kind === "frames").flatMap((c) => c.refs.filter((r) => !r.off));
  const videoOf = (card: Card) => (card.kind === "video" ? card : board.cards.find((c) => c.id === card.parent));

  /** The style a card follows, if one is picked on it or on its Video card. */
  const styleFor = (card: Card) => styles.find((s) => s.name === styleOf(card));

  /** Writes the script only, from the video card's request and any frames, then stops. */
  function writeScript(video: Card, again = false) {
    const frames = framesOf(video);
    void sendCard(video, prompts.writeScript(video.text.trim(), { again, styled: !!styleFor(video), current: again ? scriptOf(video)?.text.trim() : undefined, frames: frames.length > 0 }), frames);
  }

  /** Step one of a video: the agent draws one SVG per script line, styled on the frames. */
  function makeStoryboard(video: Card) {
    const script = scriptOf(video);
    const frames = framesOf(video);
    void sendCard(video, prompts.makeStoryboard(video.text.trim(), script?.text.trim(), { styled: !!styleFor(video), frames: frames.length > 0 }), frames);
  }

  /** Confirming the script locks it in and draws (or redraws) its storyboard. */
  function confirmScript(script: Card) {
    const video = videoOf(script);
    script.confirmed = script.text;
    if (video) makeStoryboard(video);
  }

  function redoScript(script: Card) {
    const video = videoOf(script);
    if (video) writeScript(video, true);
  }

  function redoBeat(storyboard: Card, i: number) {
    const beat = storyboard.beats?.[i];
    if (!beat) return;
    void sendCard(storyboard, prompts.redoBeat(i + 1, beat.line));
  }

  /**
   * A follow-up on a finished video:
   * - "visuals": scenes and motion change; the session's voice is attached and replaces any silent or placeholder track.
   * - "words": the script and storyboard change; when the agent stops, the canvas re-voices and rebuilds.
   * - "rebuild": a new storyboard from the confirmed script, then the canvas rebuilds the video with the session's voice.
   * - "voice": only puts the session's voiceover into a video built without it.
   */
  async function followUp(preview: Card, request: string, mode: prompts.Mode) {
    const video = videoOf(preview);
    const storyboard = video && board.cards.find((c) => c.kind === "storyboard" && c.parent === video.id);
    const styled = !!(video && styleFor(video));
    if (mode === "words" || mode === "rebuild") {
      await sendCard(preview, prompts.followUp(mode, request, { styled }));
      // Set only once the agent is working, so the rebuild waits for it to finish.
      if (storyboard && board.active !== undefined) board.followUp = storyboard.id;
      return;
    }
    const voice = await voiceBlock(scriptOf(video ?? preview));
    if (!voice) {
      preview.status = "Make the voiceover on the script card first with Generate. The video only uses the voiceover made in this session.";
      return;
    }
    if (video) video.voicedBuild = true;
    void sendCard(preview, prompts.followUp(mode, request, { styled, voice }));
  }

  /** Picks up a run that stopped after a tool result without replying. */
  function continueRun(card: Card) {
    void sendCard(card, prompts.continueRun(card.stopped ?? "an unknown step"));
  }

  /** The length of an audio clip, or undefined if it can't be read. */
  const audioLength = (src: string) =>
    new Promise<number | undefined>((done) => {
      const audio = new Audio();
      audio.preload = "metadata";
      audio.onloadedmetadata = () => done(Number.isFinite(audio.duration) ? audio.duration : undefined);
      audio.onerror = () => done(undefined);
      audio.src = src;
    });

  /** Compares the built video, as video_preview measured it, with the session's voiceover and storyboard. */
  async function checkSync(details: { duration?: number; scenes?: number } | undefined) {
    const preview = board.cards.find((c) => c.kind === "preview" && c.parent === board.active) ?? [...board.cards].reverse().find((c) => c.kind === "preview");
    const video = preview && board.cards.find((c) => c.id === preview.parent);
    if (!preview || !video || !details) return;
    const speech = voiced(scriptOf(video)) ? scriptOf(video)!.speech! : undefined;
    const lengths = speech ? await Promise.all(speech.clips?.length ? speech.clips.map((c) => audioLength(c.audio)) : [audioLength(speech.audio!)]) : [];
    const voice = lengths.every((l) => l !== undefined) ? lengths.reduce<number>((a, l) => a + l!, 0) : undefined;
    const storyboard = board.cards.find((c) => c.kind === "storyboard" && c.parent === video.id);
    preview.sync = outOfSync(details, voice || undefined, storyboard?.beats?.length);
  }

  /** Asks the agent to re-time the video to its voiceover and storyboard. */
  function fixSync(preview: Card) {
    void followUp(preview, prompts.fixSync(preview.sync ?? ""), "visuals");
  }

  /**
   * This session's voiceover as instructions for the agent: per-scene clips, or one
   * clip to cut at scene boundaries. Null when no voice has been made from the script.
   */
  async function voiceBlock(script: Card | undefined): Promise<string | null> {
    if (!voiced(script)) return null;
    const clips = script!.speech!.clips ?? [];
    const paths = await Promise.all(clips.map(async (c, i) => ({ scene: c.scene, path: await toDisk(`scene-${String(i + 1).padStart(2, "0")}.mp3`, c.audio) })));
    return prompts.voiceover(paths, clips.length ? undefined : await toDisk("voiceover.mp3", script!.speech!.audio!));
  }

  async function skipQuestions(card: Card) {
    const parent = board.cards.find((c) => c.id === card.parent);
    await sendCard(card, prompts.SKIP);
    if (card.status?.startsWith("Sent")) {
      if (parent) parent.status = "Skipped the questions. The agent is carrying on.";
      removeCard(card);
    }
  }

  /** Step two: the confirmed storyboard and the voiceover become a HyperFrames video. */
  async function confirmStoryboard(card: Card) {
    const video = board.cards.find((c) => c.id === card.parent);
    const script = video && scriptOf(video);
    const style = video && styleFor(video);
    if (style) return buildStyled(card, style, script);
    const audio = video && voiced(scriptOf(video)) ? scriptOf(video)!.speech!.audio : undefined;
    try {
      const beats = await Promise.all((card.beats ?? []).map(async (b, i) => {
        const path = await saveReference(`beat-${String(i + 1).padStart(2, "0")}.svg`, new TextEncoder().encode(b.svg));
        return `${i + 1}. "${b.line}"\n   ${path}`;
      }));
      const voice = audio ? await toDisk("voiceover.mp3", audio) : null;
      if (video) video.voicedBuild = !!voice;
      const rebuild = board.cards.some((c) => c.kind === "preview" && c.parent === card.parent);
      void sendCard(card, prompts.build(beats, { rebuild, voice: voice ?? undefined }));
    } catch (e) {
      card.status = message(e);
    }
  }

  /**
   * A styled build follows the style's own workflow from its voice step: the
   * canvas made one ElevenLabs clip per scene, and the agent normalises,
   * transcribes and builds them with the style's generator.
   */
  async function buildStyled(storyboard: Card, style: Style, script: Card | undefined) {
    // The episode is never built on silent stand-ins: it needs this session's voice.
    const voice = await voiceBlock(script).catch(() => null);
    if (!voice) {
      storyboard.status = "Make the voiceover on the script card first with Generate, or connect ElevenLabs. The video isn't built without it.";
      return;
    }
    const video = board.cards.find((c) => c.id === storyboard.parent);
    if (video) video.voicedBuild = true;
    try {
      const plan = (storyboard.beats ?? []).map((b, i) => `- scene ${i + 1}: ${b.component ?? "no component named yet; pick an unused one from the catalogue"}`);
      const rebuild = board.cards.some((c) => c.kind === "preview" && c.parent === storyboard.parent);
      void sendCard(storyboard, prompts.buildStyled(style.name, plan, { rebuild, voice }));
    } catch (e) {
      storyboard.status = message(e);
    }
  }

  /** Sends the answers, closes the questions and lets the agent carry on. */
  async function answer(card: Card) {
    const parent = board.cards.find((c) => c.id === card.parent);
    await sendCard(card, prompts.answers(answers(card)));
    if (card.status?.startsWith("Sent")) {
      if (parent) parent.status = "Answered. The agent is carrying on.";
      removeCard(card);
    }
  }

  function renderVideo(card: Card) {
    void sendCard(card, prompts.RENDER);
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
      // Ask the engine which session it has open: writing that one's file
      // directly would be undone by the engine's next save of it.
      const open = await request({ type: "get_state" }).then((s) => s.sessionFile, () => null);
      if (open === session) await request({ type: "set_session_name", name });
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

<div class="layout" style:--sidebar-width="{sidebarWidth}px">
  {#if sidebarOpen}
  <Sidebar
    {home}
    groups={recent.map((path) => ({ folder: path, sessions: saved[path] ?? [] }))}
    current={pending ?? sessionFile}
    {busy}
    onnew={newSession}
    onresume={(path, session) => { page = "chat"; void open(path, session); }}
    onrename={rename}
    ondelete={remove}
    onaddfolder={addFolder}
    onremovefolder={removeFolder}
    oncollapse={toggleSidebar}
    oncanvas={() => (page = "canvas")}
    canvas={page === "canvas"}
    onsettings={() => { settingsStart = "providers"; page = "settings"; }}
    onaccount={() => { settingsStart = "account"; page = "settings"; }}
    {account}
    onsignin={showSignIn}
    onsignout={rekaSignOut}
  />
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resizer" onpointerdown={resizeSidebar} ondblclick={() => { sidebarWidth = 260; localStorage.removeItem(SIDEBAR_WIDTH); }} title="Drag to resize, double-click to reset"></div>
  {/if}
  <main>
    <div class="titlebar" class:collapsed={!sidebarOpen} data-tauri-drag-region>
      {#if !sidebarOpen}
        <button class="iconbtn toggle" onclick={toggleSidebar} title="Show sidebar" aria-label="Show sidebar"><Icon name="sidebar" /></button>
      {/if}
      <span class="grow ellipsis" data-tauri-drag-region>{page === "settings" ? "Settings" : page === "canvas" ? "Canvas" : title}</span>
      {#if page === "chat" && folder && inside}
        <button class="iconbtn on" title={`In WSL: ${inside}`} aria-label="WSL" aria-disabled="true"><Icon name="terminal" /></button>
      {:else if page === "chat" && folder && distros.length === 1}
        <button class="iconbtn" class:on={!!connected} onclick={() => connectWsl(connected ? null : distros[0].name)} title={connected ? `WSL: ${connected}. Click to disconnect` : `Connect to WSL (${distros[0].name})`} aria-label="WSL" aria-pressed={!!connected}><Icon name="terminal" /></button>
      {:else if page === "chat" && folder && distros.length > 1}
        <span class="wsl" class:on={!!connected}>
          <Menu
            actions
            label="WSL"
            title={connected ? `WSL: ${connected}` : "Connect to WSL"}
            options={[
              ...distros.map((d) => ({ value: d.name, label: d.name === connected ? `${d.name} (connected)` : d.default ? `${d.name} (default)` : d.name })),
              ...(connected ? [{ value: "", label: "Disconnect" }] : []),
            ]}
            onpick={(d) => connectWsl(d || null)}
          >
            {#snippet trigger()}<Icon name="terminal" />{/snippet}
          </Menu>
        </span>
      {/if}
      {#if page === "chat" && fromCanvas}
        <button class="iconbtn" onclick={() => (page = "canvas")} title="Open this session's canvas" aria-label="Open canvas"><Icon name="layout" /></button>
      {/if}
      {#if page === "chat" && folder}
        <button class="iconbtn" class:on={showBrowser} onclick={() => (browserOpen ? closeBrowser() : (browserOpen = true))} title={showBrowser ? "Hide browser" : "Show browser"} aria-label="Browser" aria-pressed={showBrowser}><Icon name="globe" /></button>
      {/if}
      {#if active}
        <button class="iconbtn" class:on={panelOpen} onclick={() => (panelOpen = !panelOpen)} title={panelOpen ? "Hide session details" : "Show session details"} aria-label="Session details" aria-pressed={panelOpen}><Icon name="panel" /></button>
      {/if}
    </div>
    <UpdateBanner {busy} onrestart={() => closeProject().catch(() => {})} />
    <div class="body">
  {#if page === "settings"}
    {#key settingsStart}
      <Settings onback={() => (page = "chat")} onsaved={signedIn} onsignin={showSignIn} onsignout={rekaSignOut} {account} onaccount={(a) => (account = a)} start={settingsStart} />
    {/key}
  {:else if page === "canvas"}
    <Canvas {model} reasoning={{ thinking, levels }} onsend={(card) => sendCard(card)} canSend={!!folder} onwrite={(card) => writeScript(card)} onredoscript={redoScript} onconfirmscript={confirmScript} onredostoryboard={(card) => { const v = videoOf(card); if (v) makeStoryboard(v); }} onredobeat={redoBeat} onconfirm={confirmStoryboard} onrender={renderVideo} onfollowup={followUp} oncontinue={continueRun} onfixsync={fixSync} {styles} onanswer={answer} onskip={skipQuestions} onstop={() => post({ type: "abort" }).catch(() => {})} onopenvideo={(path) => folder && openFile(folder, path).catch((e) => notice(message(e)))} onconnect={() => { settingsStart = "connectors"; page = "settings"; }} />
  {:else}
    <div class="center">
    {#if error}<div class="banner error">{error}</div>{/if}
    {#if loading}
      <div class="chat" aria-busy="true" aria-label="Loading session">
        {#each [40, 26, 34] as width}
          <div class="user bone" style:width="{width}%"></div>
          <div class="bones">{#each [96, 100, 62] as w}<div class="bone line" style:width="{w}%"></div>{/each}</div>
        {/each}
      </div>
    {:else if !chat.items.length && !chat.running}
      <NewSession {folder} {recent} {busy} onopen={(p) => open(p)} onchoose={choose} onremove={removeFolder} onproviders={hasAccess ? undefined : () => (page = "settings")} />
    {:else}
      {#if !hasAccess}
        <div class="banner">No provider is signed in. <button class="link" onclick={() => (page = "settings")}>Add a provider in Settings</button></div>
      {/if}
      {#key sessionFile}<ChatView {chat} {folder} onolder={older} onresend={(text) => { chat = { ...chat, rejected: chat.rejected.filter((t) => t !== text) }; send(text, [], false); }} />{/key}
    {/if}
    {#if folder}
      <div class="dock">
        <Composer bind:this={composer} running={chat.running} disabled={opening} {chat} onsend={send} onabort={() => post({ type: "abort" }).catch(() => {})} />
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
  {#if loading && panelOpen}
    <aside class="right" aria-busy="true" aria-label="Loading session details">
      {#each [70, 45, 85, 55, 65] as width}<div class="bone line" style:width="{width}%"></div>{/each}
    </aside>
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
  {#if showBrowser}
    <BrowserPane
      folder={folder!}
      {pointer}
      {poke}
      wide={browserWide}
      running={chat.running}
      onnotes={(text, images) => composer?.add(text, images)}
      onwide={() => (browserWide = !browserWide)}
      onclose={closeBrowser}
    />
  {/if}
    </div>
  </main>
</div>

{#if gate === null}
  <div style="position: fixed; inset: 0; z-index: 50; background: var(--bg);"></div>
{:else if gate}
  <SignIn onsignedin={() => { gate = false; page = "chat"; void loadAccount(); }} onskip={() => { localStorage.setItem(OWN_PROVIDER, "1"); gate = false; }} />
{/if}

{#if dialog === "models"}
  <Models current={model} onclose={() => (dialog = null)} onchosen={async () => { dialog = null; await refresh(); }} />
{/if}
