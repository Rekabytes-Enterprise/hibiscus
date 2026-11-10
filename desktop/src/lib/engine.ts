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
};
export type ModelInfo = {
  provider: string;
  id: string;
  contextWindow: number | null;
  reasoning: boolean;
  images: boolean;
};

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
  for (const waiter of pending.values()) waiter.reject(new Error(payload));
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
  const id = await invoke<string>("send", { command });
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

export const openProject = async (folder: string, latest: boolean, session?: string) => {
  await ready;
  await invoke<void>("open_project", { folder, latest, session: session ?? null });
};
export const closeProject = () => invoke<void>("close_project");
export const listSessions = (folder: string) => invoke<SavedSession[]>("sessions", { folder });
/** Names a session no engine has open; use `set_session_name` for the open one. */
export const renameSession = (path: string, name: string) => invoke<void>("rename_session", { path, name });
export const deleteSession = (path: string) => invoke<void>("delete_session", { path });
export const listProviders = () => invoke<ProviderInfo[]>("providers");
export const saveApiKey = (provider: string, key: string) =>
  invoke<void>("save_api_key", { provider, key });
export const signOut = (provider: string) => invoke<void>("sign_out", { provider });
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
export type Changes = { base: string | null; bases: string[]; branch: string | null; diff: string; truncated: boolean };
/** `base` is the branch the user picked; `null` lets the app pick the nearest one. */
export const gitChanges = (folder: string, base: string | null = null) => invoke<Changes | null>("git_changes", { folder, base });
export const openFile = (folder: string, path: string) => invoke<void>("open_file", { folder, path });
