// The canvas cards, kept outside the page so they survive switching back to chat
// and so a chat tool call can open cards on it.

export type Model = { provider: string; id: string };
/** "frames" cards are made by the analyze_video tool, not picked by the user. */
export type Kind = "video" | "image" | "speech" | "frames" | "storyboard" | "preview" | "question";
/** An image or video for the model to follow: a file the user added (with a preview URL) or a path the agent named. */
export type Reference = {
  video: boolean;
  name: string;
  url?: string;
  /** A still from a video, made in the app because the web view won't paint one from the file. */
  poster?: string;
  /** True until the file's preview is ready. */
  loading?: boolean;
  /** A frame the user switched off as a style reference. */
  off?: boolean;
  /** Where the file is on disk, once saved for the agent or named by it. */
  path?: string;
  /** A frame's moment in its video, such as "0:03.5". */
  time?: string;
};
/** `parent` joins a card to the one it belongs to, such as a script to its video. */
export type Card = {
  id: number;
  kind: Kind;
  text: string;
  model: Model | null;
  /** The card's reasoning level and the levels its model offers. */
  thinking?: string;
  levels?: string[];
  refs: Reference[];
  x: number;
  y: number;
  parent?: number;
  /** The agent's questions, each with suggested answers and the one picked or typed. */
  questions?: { question: string; options: string[]; answer?: string; other?: boolean }[];
  /** A storyboard's beats: each script line with the SVG the agent drew for it. */
  beats?: { line: string; svg: string; component?: string }[];
  /** A video preview card's local page, and the rendered file once there is one. */
  url?: string;
  video?: string;
  /** Bumped to reload the preview page. */
  reload?: number;
  /** Whether the last build of this video had the session's voice. */
  voicedBuild?: boolean;
  /** While the agent works for this card: when it started and what it's doing. */
  started?: number;
  activity?: string;
  /** A Video card's style: the name of the style skill every step follows. */
  style?: string;
  /** What happened after Send, shown under the card. */
  status?: string;
  /** The last step of a run that ended without a final reply, so it can be continued. */
  stopped?: string;
  /** A preview card's mismatch with its voiceover or storyboard, such as "The video is 118.0 s; the voiceover is 96.2 s". */
  sync?: string;
  /** The analyze_video call that fills a Frames card. */
  call?: string;
  /** Stacking order: the card last moved sits on top. */
  z?: number;
  /** A size the user dragged the card to. */
  w?: number;
  h?: number;
  /** Text to speech: the ElevenLabs model and voice, the audio once made, and the text it was made from. */
  speech?: { model?: string; voice?: string; audio?: string; spoken?: string; busy?: boolean; error?: string; clips?: { scene: string; audio: string }[] };
  /** A script's text when the user confirmed it. */
  confirmed?: string;
  /** The script text a storyboard or preview was made from, to tell when it is out of date. */
  script?: string;
};

export const WIDTH = 380;
/** Text to speech cards are wider, for the script and the voice controls. */
export const widthOf = (card: Card) => card.w ?? (card.kind === "speech" ? 440 : WIDTH);
/** `active` is the card last sent to the agent: cards its tools open join to it. */
export const board = $state<{ cards: Card[]; active?: number; followUp?: number }>({ cards: [] });
let next = 0;

/**
 * The canvas as saved with its session. Blob URLs only live in this window, so
 * a reference keeps its saved path instead and is read back on restore; work in
 * flight (spinners, "Sending…") is dropped.
 */
export function serialize(): string {
  if (!board.cards.length) return "";
  const cards = board.cards.map((c) => ({
    ...c,
    refs: c.refs.map((r) => ({ ...r, url: r.url?.startsWith("blob:") ? undefined : r.url, loading: undefined })),
    speech: c.speech && { ...c.speech, busy: undefined },
    status: c.status === "Sending…" ? undefined : c.status,
  }));
  return JSON.stringify({ version: 1, cards });
}

/** Puts a saved canvas back, or clears it; `read` loads saved reference files for their previews. */
export function restore(json: string | null, read?: (path: string) => Promise<ArrayBuffer>) {
  let cards: Card[] = [];
  try {
    cards = json ? (JSON.parse(json).cards ?? []) : [];
  } catch {}
  board.cards = cards;
  board.active = undefined;
  next = Math.max(0, ...cards.map((c) => c.id + 1));
  if (!read) return;
  for (const card of board.cards) {
    for (const ref of card.refs) {
      if (ref.url || !ref.path || ref.video) continue;
      ref.loading = true;
      void read(ref.path)
        .then((bytes) => (ref.url = URL.createObjectURL(new Blob([bytes]))))
        .catch(() => {})
        .finally(() => (ref.loading = false));
    }
  }
}

/** Adds a card, each one a step down and right of the last so they don't cover each other. */
export function addCard(model: Model | null, card: Partial<Card> = {}, reasoning?: { thinking: string; levels: string[] }): Card {
  const step = board.cards.length % 8;
  board.cards.push({ id: next++, kind: "image", text: "", model: model && { ...model }, thinking: reasoning?.thinking, levels: reasoning?.levels && [...reasoning.levels], refs: [], x: 40 + step * 28, y: 64 + step * 36, ...card });
  return board.cards[board.cards.length - 1];
}

const VIDEO = /\.(mp4|mov|webm|mkv|avi|m4v)$/i;
const fileName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/**
 * Opens a video's voiceover script from the text_to_speech tool: a Video card
 * with its references, and a Text to speech card holding the script, joined to it.
 */
/** The card the agent is working for, or a new Video card for a request made in chat. */
function videoCard(model: Model | null, reasoning: { thinking: string; levels: string[] } | undefined, args: { video?: string; references?: string[] }, reuse = false): Card {
  const active = board.cards.find((c) => c.id === board.active);
  if (active) return active;
  // A storyboard or preview from the chat belongs to the latest video.
  const latest = reuse ? [...board.cards].reverse().find((c) => c.kind === "video") : undefined;
  if (latest) return latest;
  const bottom = Math.max(24, ...board.cards.map((c) => c.y + 260));
  return addCard(model, {
    kind: "video",
    text: args.video ?? "",
    refs: (args.references ?? []).map((path) => ({ video: VIDEO.test(path), name: fileName(path) })),
    x: 40,
    y: board.cards.length ? bottom : 64,
  }, reasoning);
}

/** A free spot to the right of `parent`, below any card already joined to it. */
function besides(parent: Card) {
  const x = parent.x + widthOf(parent) + 80;
  const taken = board.cards.filter((c) => c.parent === parent.id);
  return { x, y: Math.max(parent.y, ...taken.map((c) => c.y + (c.h ?? 300) + 40)) };
}

export function openScript(model: Model | null, args: { script?: string; video?: string; references?: string[] }, reasoning?: { thinking: string; levels: string[] }) {
  const video = videoCard(model, reasoning, args);
  // A redo replaces the script in place; its voice choice stays, and its audio now reads as out of date.
  const old = scriptOf(video);
  if (old) {
    old.text = args.script ?? "";
    old.confirmed = undefined;
    old.status = undefined;
    return;
  }
  addCard(model, { kind: "speech", text: args.script ?? "", ...besides(video), parent: video.id }, reasoning);
}

/** Whether a script's voiceover (one clip, or one per scene) was made from its current text. */
export const voiced = (script: Card | undefined) =>
  !!script?.speech && script.speech.spoken === script.text && (!!script.speech.audio || !!script.speech.clips?.length);

/** The style a card follows: its own pick, else that of the card it belongs to. */
export function styleOf(card: Card | undefined): string | undefined {
  if (!card) return undefined;
  return card.style ?? board.cards.find((c) => c.id === card.parent)?.style;
}

/**
 * A styled script's scenes. The agent writes one `## N. Scene name` heading per
 * scene; only the lines under each heading are spoken.
 */
export function scenes(text: string): { scene: string; lines: string }[] {
  // Without headings, each blank-line paragraph is a scene.
  if (!/^##\s+/m.test(text)) {
    const paragraphs = text.split(/\n\s*\n/).map((p) => p.trim()).filter(Boolean);
    return paragraphs.map((lines, i) => ({ scene: paragraphs.length > 1 ? `Scene ${i + 1}` : "Script", lines }));
  }
  const parts = text.split(/^##\s+/m).filter((p) => p.trim());
  return parts.map((part) => {
    const [head, ...rest] = part.split("\n");
    return { scene: head.trim(), lines: rest.join("\n").trim() };
  }).filter((s) => s.lines);
}

/** Whether `card` (a storyboard or preview) was made from an older script than the one now joined. */
export function stale(card: Card) {
  const video = board.cards.find((c) => c.id === card.parent);
  const script = video && scriptOf(video);
  return !!script && card.script !== undefined && card.script !== script.text;
}

/** Components used by more than one beat, so the storyboard can flag repeats. */
export function repeated(card: Card): string[] {
  const seen = new Map<string, number>();
  for (const b of card.beats ?? []) {
    const name = b.component?.trim().toLowerCase();
    if (name) seen.set(name, (seen.get(name) ?? 0) + 1);
  }
  return [...seen].filter(([, n]) => n > 1).map(([name]) => name);
}

/** Whether the agent is working for this card or the card it belongs to. */
export const busy = (card: Card) => board.active !== undefined && (board.active === card.id || board.active === card.parent);

/** The script the agent wrote for `card`: its joined Text to speech card. */
export const scriptOf = (card: Card) => board.cards.find((c) => c.parent === card.id && c.kind === "speech");

/** Questions from the ask_user tool, joined to the card the agent is working for. */
export function openQuestions(args: { questions?: { question: string; options: string[] }[] }) {
  const card = videoCard(null, undefined, {}, true);
  addCard(null, { kind: "question", text: "Questions", questions: (args.questions ?? []).map((q) => ({ ...q })), ...besides(card), parent: card.id, w: 420 });
}

/** The answers as the user's reply, one question and answer per line pair. */
export const answers = (card: Card) => (card.questions ?? []).map((q) => `- ${q.question}\n  ${q.answer?.trim()}`).join("\n");

/** The storyboard from the storyboard tool, joined to the card being worked on. */
export function openStoryboard(args: { title?: string; beats?: { line: string; svg: string; component?: string }[]; from?: number }) {
  const video = videoCard(null, undefined, {}, true);
  const script = scriptOf(video)?.text;
  const old = board.cards.find((c) => c.kind === "storyboard" && c.parent === video.id);
  // A later batch (`from` 5, 9, …) fills in from that beat; the first batch starts a fresh storyboard.
  const from = Math.max(1, args.from ?? 1);
  if (old && from > 1) {
    const beats = [...(old.beats ?? [])];
    beats.splice(from - 1, args.beats?.length ?? 0, ...(args.beats ?? []));
    Object.assign(old, { beats, script, status: undefined });
    return;
  }
  if (old) {
    Object.assign(old, { text: args.title || old.text, beats: args.beats ?? [], script, status: undefined });
    return;
  }
  addCard(null, { kind: "storyboard", text: args.title || "Storyboard", beats: args.beats ?? [], script, ...besides(video), parent: video.id, w: 480, h: 380 });
}

/** The video preview from video_preview: one card per video, updated as the agent renders. */
export function openPreview(args: { url?: string; video?: string }) {
  const video = videoCard(null, undefined, {}, true);
  const shown = board.cards.find((c) => c.kind === "preview" && c.parent === video.id);
  const script = scriptOf(video)?.text;
  if (shown) {
    // A new URL means a rebuilt video: its old render no longer matches.
    if (args.url && args.url !== shown.url) shown.video = undefined;
    Object.assign(shown, { url: args.url ?? shown.url, video: args.video ?? shown.video, script, status: undefined, sync: undefined, reload: (shown.reload ?? 0) + 1 });
    return;
  }
  addCard(null, { kind: "preview", text: "Video preview", url: args.url, video: args.video, script, ...besides(video), parent: video.id, w: 560, h: 400 });
}

/** A Frames card for an analyze_video call, waiting for its frames. */
export function openFrames(call: string, path: string) {
  const video = videoCard(null, undefined, { references: [path] });
  addCard(null, { kind: "frames", text: fileName(path), call, ...besides(video), parent: video.id, w: 420 });
}

const clock = (s: number) => `${Math.floor(s / 60)}:${(s % 60).toFixed(1).padStart(4, "0")}`;

/** Fills a Frames card from the tool's result: its images, and their moments from `details.frames`. */
export function fillFrames(call: string, result: any, failed: boolean) {
  const card = board.cards.find((c) => c.call === call);
  if (!card) return;
  if (failed) {
    card.status = result?.content?.find((c: any) => c.type === "text")?.text ?? "The frames could not be cut.";
    return;
  }
  const times: number[] = result?.details?.frames ?? [];
  const images = (result?.content ?? []).filter((c: any) => c.type === "image");
  card.refs = images.map((image: any, i: number) => ({
    video: false,
    name: `${card.text} at ${clock(times[i] ?? 0)}`,
    url: `data:${image.mimeType};base64,${image.data}`,
    time: clock(times[i] ?? 0),
  }));
  card.status = undefined;
}

export function removeCard(card: Card) {
  for (const ref of card.refs) if (ref.url) URL.revokeObjectURL(ref.url);
  board.cards = board.cards.filter((c) => c.id !== card.id);
  for (const c of board.cards) if (c.parent === card.id) c.parent = undefined;
}

/**
 * How a built video differs from what it was made from: its length against the
 * voiceover (a short tail is fine; cutting into the voice or running long is not)
 * and its scene count against the storyboard's beats. Undefined when it matches.
 */
export function outOfSync(video: { duration?: number; scenes?: number }, voice?: number, beats?: number): string | undefined {
  const problems: string[] = [];
  if (video.duration !== undefined && voice && (video.duration < voice - 0.5 || video.duration > voice + 3)) {
    problems.push(`The video is ${video.duration.toFixed(1)} s; the voiceover is ${voice.toFixed(1)} s`);
  }
  if (video.scenes && beats && video.scenes !== beats) problems.push(`it has ${video.scenes} scenes and the storyboard has ${beats}`);
  if (!problems.length) return undefined;
  const text = problems.join(", and ");
  return text[0].toUpperCase() + text.slice(1) + ".";
}
