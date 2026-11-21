// Calls into the Rust glue (src-tauri/src/main.rs). Engine records are the
// ones documented in docs/desktop/protocol.md.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Record = { type: string; [field: string]: any };

export type SavedSession = { path: string; title: string; modified: number };
export type ProviderInfo = {
  id: string;
  apiKey: boolean;
  /** Signs in through the browser (Codex). */
  browser: boolean;
  signedIn: boolean;
  keyPage: string | null;
  /** A local AI server added in Settings, which Settings can remove. */
  local: boolean;
};
/** A model server on this computer or the home network, such as Ollama or LM Studio. */
export type LocalProvider = {
  id: string;
  baseUrl: string;
  api: "chat_completions" | "responses" | "anthropic";
  models: string[];
  contextWindow: number | null;
  /** Empty when the server takes no key. */
  key: string;
};
export type ModelInfo = {
  provider: string;
  id: string;
  contextWindow: number | null;
  reasoning: boolean;
  images: boolean;
  /** The reasoning levels this model offers; the first is the default. */
  thinkingLevels?: string[];
};

class ProjectUnavailable extends Error {}

const pending = new Map<string, { resolve: (data: any) => void; reject: (e: Error) => void }>();
// Responses that arrived before `request` learned their id.
const early = new Map<string, Record>();
const listeners = new Set<(event: Record) => void>();

// Registration is async; commands wait for it so no early response is lost.
const ready = Promise.all([
listen<Record>("engine", ({ payload }) => {
  if (payload.type === "response" && typeof payload.id === "string" && payload.id.startsWith("desktop-")) {
    const waiter = pending.get(payload.id);
    pending.delete(payload.id);
    if (waiter) settle(payload, waiter);
    else early.set(payload.id, payload);
    return;
  }
  for (const listener of listeners) listener(payload);
}),

listen<string>("engine-stopped", ({ payload }) => {
  for (const waiter of pending.values()) waiter.reject(new ProjectUnavailable(payload));
  pending.clear();
  early.clear();
  for (const listener of listeners) listener({ type: "engine_stopped", error: payload });
}),
]);

/** Engine events other than command responses. Returns an unsubscribe function. */
export function onEvent(listener: (event: Record) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Sends a command and waits for its response data. */
export async function request(command: Record): Promise<any> {
  await ready;
  const id = await invoke<string>("send", { command }).catch((error: unknown) => {
    if (error === "No project is open." || error === "The engine has stopped.") {
      throw new ProjectUnavailable(error);
    }
    throw error;
  });
  return new Promise((resolve, reject) => {
    const response = early.get(id);
    early.delete(id);
    if (response) settle(response, { resolve, reject });
    else pending.set(id, { resolve, reject });
  });
}

function settle(response: Record, waiter: { resolve: (data: any) => void; reject: (e: Error) => void }) {
  if (response.success) waiter.resolve(response.data ?? {});
  else waiter.reject(new Error(response.error ?? "Command failed"));
}

/**
 * Sends a command whose effects arrive as events (prompt, abort). The
 * response only says it was accepted; failures still reject.
 */
export async function post(command: Record): Promise<void> {
  await request(command);
}

/** True when `session` was still running in the background and came back as it is. */
export const openProject = async (folder: string, latest: boolean, session?: string) => {
  await ready;
  return invoke<boolean>("open_project", { folder, latest, session: session ?? null });
};
/**
 * Keeps the open engine running in the background while its turn finishes, so
 * another session can open. Returns the id its `onBackground` events carry, or
 * null when no turn is running.
 */
export const parkProject = (session: string) => invoke<number | null>("park_project", { session });
/** Events from engines running in the background, with the id `parkProject` returned. */
export const onBackground = (listener: (id: number, event: Record) => void) =>
  listen<{ id: number; event: Record }>("engine-background", (e) => listener(e.payload.id, e.payload.event));
/** Quitting would stop `count` running turns; the window asks, then calls `quit`. */
export const onConfirmQuit = (listener: (count: number) => void) => listen<number>("confirm-quit", (e) => listener(e.payload));
export const quit = () => invoke<void>("quit");
export const closeProject = () => invoke<void>("close_project");
export const listSessions = (folder: string) => invoke<SavedSession[]>("sessions", { folder });
/** Names a session no engine has open; use `set_session_name` for the open one. */
export const renameSession = (path: string, name: string) => invoke<void>("rename_session", { path, name });
export const deleteSession = (path: string) => invoke<void>("delete_session", { path });
export const listProviders = () => invoke<ProviderInfo[]>("providers");
export const saveApiKey = (provider: string, key: string) =>
  invoke<void>("save_api_key", { provider, key });
export const signOut = (provider: string) => invoke<void>("sign_out", { provider });
export const addLocalProvider = (provider: LocalProvider) => invoke<void>("add_local_provider", { provider });
export const removeLocalProvider = (id: string) => invoke<void>("remove_local_provider", { id });
/** Whether OpenRouter requests ask for providers that don't store or train on prompts. */
export const trainingDenied = () => invoke<boolean>("training_denied");
export const setTrainingDenied = (deny: boolean) => invoke<void>("set_training_denied", { deny });
/** Opens the Codex sign-in page; returns its link. Watch `onCodexLogin` for the result. */
export const codexStart = () => invoke<string>("codex_start");
export const codexPaste = (url: string) => invoke<void>("codex_paste", { url });
export const codexCancel = () => invoke<void>("codex_cancel");
/** `null` once signed in, else the error. */
export const onCodexLogin = (listener: (error: string | null) => void) =>
  listen<string | null>("codex-login", (e) => listener(e.payload));
/** The Reka Bytes account for Rooxcode models; its tokens never reach the window. */
/** `avatar` is the pixel character picked on the Rooxcode account page; `null` is the default. */
export type RekaAccount = { name: string | null; email: string | null; avatar: string | null };
export const rekaAccount = () => invoke<RekaAccount | null>("reka_account");
/** Like `rekaAccount`, with the name and avatar reloaded from Rooxcode first. */
export const rekaProfile = () => invoke<RekaAccount | null>("reka_profile");
/** A budget window in percent; `dropsAt` is Unix seconds, `retryAfter` seconds while full. */
export type RekaWindow = { percent: number; dropsAt: number | null; retryAfter: number | null };
/** The Account page: a window is `null` when its limit is off. */
export type RekaUsage = {
  account: RekaAccount;
  fiveHours: RekaWindow | null;
  week: RekaWindow | null;
  credits: number | null;
  device: string | null;
};
export const rekaUsage = () => invoke<RekaUsage>("reka_usage");
/** The error a Rooxcode prompt gets without a session. */
export const rekaSignInError = () => invoke<string>("reka_sign_in_error");
/** Opens Reka Bytes sign-in in the browser; returns its link. Watch `onRekaLogin` for the result. */
export const rekaStart = () => invoke<string>("reka_start");
export const rekaPaste = (url: string) => invoke<void>("reka_paste", { url });
export const rekaCancel = () => invoke<void>("reka_cancel");
/** `null` once signed in, else the error. */
export const onRekaLogin = (listener: (error: string | null) => void) =>
  listen<string | null>("reka-login", (e) => listener(e.payload));
export type Changes = { base: string | null; bases: string[]; branch: string | null; diff: string; truncated: boolean };
/** `base` is the branch the user picked; `null` lets the app pick the nearest one. */
export const gitChanges = (folder: string, base: string | null = null) => invoke<Changes | null>("git_changes", { folder, base });
export const openFile = (folder: string, path: string) => invoke<void>("open_file", { folder, path });

/** A WSL 2 distro found on this computer (#201). */
export type Distro = { name: string; default: boolean };
/** WSL 2 distros, the default first; empty outside Windows or without WSL. */
export const wslDistros = () =>
  invoke<Distro[]>("wsl_distros").then((d) => [...d].sort((a, b) => Number(b.default) - Number(a.default)));
/** Runs the folder's shell tool and dev server in `distro`, or back on Windows with null. */
export const wslConnect = (folder: string, distro: string | null) => invoke<void>("wsl_connect", { folder, distro });
/** The distro's home folder as a Windows path, for the folder picker. */
export const wslHome = (distro: string) => invoke<string | null>("wsl_home", { distro });

/** An ElevenLabs model that can do text to speech. */
export type VoiceModel = { id: string; name: string; description: string; maxText: number | null };
/** A voice in the user's ElevenLabs library. */
export type Voice = { id: string; name: string; category: string | null; previewUrl: string | null };

/**
 * The ElevenLabs connector. "Sign in with ElevenLabs" (OAuth, like Claude's
 * connector) or a pasted API key; credentials stay in the app's private files.
 */
export const elevenlabs = {
  /** How it is connected: "oauth", "key", or null. */
  status: () => invoke<"oauth" | "key" | null>("elevenlabs_status"),
  /** Opens the ElevenLabs sign-in page; returns its link. Watch `onLogin` for the result. */
  signIn: () => invoke<string>("elevenlabs_sign_in"),
  paste: (url: string) => invoke<void>("elevenlabs_sign_in_paste", { url }),
  cancel: () => invoke<void>("elevenlabs_sign_in_cancel"),
  /** `null` once signed in, else the error. */
  onLogin: (listener: (error: string | null) => void) => listen<string | null>("elevenlabs-login", (e) => listener(e.payload)),
  connect: (key: string) => invoke<void>("elevenlabs_connect", { key }),
  disconnect: () => invoke<void>("elevenlabs_disconnect"),
  models: () => invoke<VoiceModel[]>("elevenlabs_models"),
  voices: (search?: string) => invoke<Voice[]>("elevenlabs_voices", { search: search ?? null }),
  /** MP3 audio as base64. */
  speak: (text: string, voice: string, model: string) =>
    invoke<{ mimeType: string; data: string }>("elevenlabs_speak", { text, voice, model }),
};

/** Saves a canvas reference's bytes where the agent can read them; returns the path. */
export const saveReference = (name: string, bytes: Uint8Array) =>
  invoke<string>("canvas_save_reference", bytes, { headers: { "x-name": name.replace(/[^A-Za-z0-9._-]/g, "_") } });

/** A session's saved canvas as JSON, or null. */
export const canvasLoad = (session: string) => invoke<string | null>("canvas_load", { session });
/** Saves a session's canvas; an empty string removes it. */
export const canvasStore = (session: string, board: string) => invoke<void>("canvas_store", { session, board });
/** A saved reference's bytes, to show it again after a restart. */
export const readReference = (path: string) => invoke<ArrayBuffer>("canvas_read_reference", { path });
/** Starts a video project's preview server, or reuses the running one, and returns its address. */
export const canvasPreview = (project: string) => invoke<string>("canvas_preview", { project });

/** The video engine (Node.js, HyperFrames and its Chrome), installed from the canvas. */
export const videoEngine = {
  installed: () => invoke<boolean>("video_engine_status"),
  install: () => invoke<void>("video_engine_install"),
  onProgress: (listener: (p: { step: string; percent: number | null }) => void) =>
    listen<{ step: string; percent: number | null }>("video-engine-progress", (e) => listener(e.payload)),
};

/** A video style: a skill in ~/.hibiscus/skills marked `hibiscus-style: true`. */
export type Style = { name: string; description: string; path: string };
export const canvasStyles = () => invoke<Style[]>("canvas_styles");

/** A command the agent runs in the background (`bash` with `background: true`). */
export type Job = {
  id: number;
  label: string;
  command: string;
  /** Milliseconds since the Unix epoch. */
  started: number;
  running: boolean;
  exitCode: number | null;
  /** Stopped by the user or the agent, not by itself. */
  stopped: boolean;
  durationMs: number;
  output: string[];
};
/** The folder's background jobs, newest first. */
export const listJobs = (folder: string) => invoke<Job[]>("jobs", { folder });
export const stopJob = (id: number) => invoke<void>("job_stop", { id });
/** Forgets the folder's finished jobs. */
export const clearJobs = (folder: string) => invoke<void>("jobs_clear", { folder });

/** Keep same-folder switches fast, but reopen the project if its worker is gone. */
export async function switchProjectSession(folder: string, session?: string): Promise<boolean> {
  try {
    await request(session ? { type: "switch_session", sessionPath: session } : { type: "new_session" });
    return true;
  } catch (error) {
    if (!(error instanceof ProjectUnavailable)) throw error;
    await openProject(folder, false, session);
    return false;
  }
}
