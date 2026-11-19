// Sessions whose turn keeps running after the window opened another one.
// The engine runs on in the app; its events land here until the session shows again.
import { apply, type Chat } from "./chat";
import type { Record } from "./engine";

/** The canvas run's progress, saved when its session leaves the window: the card it works for and its last step. */
export type Run = { active?: number; followUp?: number; runError: string | null; lastStep?: string; afterTool?: string };

export type Parked = {
  folder: string;
  /** The conversation, kept live from the engine's events. */
  chat: Chat;
  /** Events the canvas replays when the session shows again. */
  events: Record[];
  run: Run;
  /** The turn settled, or the engine stopped. */
  done: boolean;
};

// What the canvas reacts to; streamed text is left out.
const CANVAS = new Set(["agent_start", "agent_settled", "tool_execution_start", "tool_execution_end", "message_end"]);

export class Background {
  parked = $state<{ [session: string]: Parked }>({});
  #ids = new Map<number, string>();

  park(session: string, id: number, folder: string, chat: Chat, run: Run) {
    this.#ids.set(id, session);
    this.parked[session] = { folder, chat, events: [], run, done: false };
  }

  /** The session an engine id was parked for. */
  sessionOf(id: number): string | undefined {
    return this.#ids.get(id);
  }

  /** Applies an event from a session's background engine. */
  receive(session: string, event: Record) {
    const p = this.parked[session];
    if (!p) return;
    p.chat = apply(p.chat, event);
    if (CANVAS.has(event.type)) p.events.push(event);
    if (event.type === "agent_settled" || event.type === "engine_stopped") p.done = true;
  }

  /** Removes a session's record as it shows again. */
  take(session: string): Parked | undefined {
    const p = this.parked[session];
    delete this.parked[session];
    return p;
  }

  /** Sessions still running a turn in the background. */
  get working(): string[] {
    return Object.keys(this.parked).filter((s) => !this.parked[s].done);
  }

  /** Whether a background turn works in `folder`, whose dev server it may use. */
  runningIn(folder: string): boolean {
    return Object.values(this.parked).some((p) => !p.done && p.folder === folder);
  }
}
