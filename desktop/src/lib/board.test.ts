import { expect, it } from "vitest";
import { addCard, board, busy, fillFrames, openFrames, openScript, openStoryboard, outOfSync, removeCard, repeated, restore, scenes, scriptOf, serialize, stale, voiced } from "./board.svelte";

it("opens a script card joined to its video card", () => {
  openScript({ provider: "rooxcode", id: "m" }, { script: "Hello.", video: "A tour", references: ["/a/shot.png", "C:\\clips\\intro.mp4"] });
  const [video, speech] = board.cards;
  expect(video).toMatchObject({ kind: "video", text: "A tour", refs: [{ video: false, name: "shot.png" }, { video: true, name: "intro.mp4" }] });
  expect(speech).toMatchObject({ kind: "speech", text: "Hello.", parent: video.id, y: video.y });
  removeCard(video);
  expect(board.cards).toHaveLength(1);
  expect(board.cards[0].parent).toBeUndefined();
});

it("joins tool cards to the card that was sent, and fills frames from the result", () => {
  board.cards = [];
  const sent = addCard(null, { kind: "video", text: "Remake this" });
  board.active = sent.id;
  openFrames("call-1", "/refs/demo.mp4");
  const frames = board.cards.find((c) => c.kind === "frames")!;
  expect(frames).toMatchObject({ parent: sent.id, text: "demo.mp4", call: "call-1", refs: [] });
  fillFrames("call-1", { content: [{ type: "text", text: "x" }, { type: "image", mimeType: "image/jpeg", data: "AAA" }], details: { frames: [3.75] } }, false);
  expect(frames.refs).toEqual([{ video: false, name: "demo.mp4 at 0:03.8", url: "data:image/jpeg;base64,AAA", time: "0:03.8" }]);
  openScript(null, { script: "Hi." });
  const script = board.cards.find((c) => c.kind === "speech")!;
  expect(script.parent).toBe(sent.id);
  expect(script.y).toBeGreaterThan(frames.y);
  fillFrames("call-2", {}, true);
  board.active = undefined;
});

it("redos replace cards in place, and edits mark later steps out of date", () => {
  board.cards = [];
  board.active = undefined;
  const video = addCard(null, { kind: "video", text: "GitHub Education" });
  board.active = video.id;
  openScript(null, { script: "One." });
  const script = scriptOf(video)!;
  script.speech = { audio: "data:audio/mpeg;base64,AA", spoken: "One." };
  expect(voiced(script)).toBe(true);
  openStoryboard({ beats: [{ line: "One.", svg: "<svg/>" }] });
  const storyboard = board.cards.find((c) => c.kind === "storyboard")!;
  expect(stale(storyboard)).toBe(false);
  expect(busy(storyboard)).toBe(true);

  const at = { x: script.x, y: script.y };
  openScript(null, { script: "Two." });
  expect(board.cards.filter((c) => c.kind === "speech")).toHaveLength(1);
  expect({ x: script.x, y: script.y, text: script.text }).toEqual({ ...at, text: "Two." });
  expect(voiced(script)).toBe(false);
  expect(stale(storyboard)).toBe(true);

  openStoryboard({ beats: [{ line: "Two.", svg: "<svg/>" }, { line: "Three.", svg: "<svg/>" }] });
  expect(board.cards.filter((c) => c.kind === "storyboard")).toHaveLength(1);
  expect(storyboard.beats).toHaveLength(2);
  expect(stale(storyboard)).toBe(false);
  board.active = undefined;
  expect(busy(storyboard)).toBe(false);
});

it("saves a canvas and brings it back, reading saved files again", async () => {
  board.cards = [];
  const video = addCard(null, { kind: "video", text: "Remake" });
  video.refs = [
    { video: false, name: "a.png", url: "blob:gone", path: "/refs/a.png", loading: true },
    { video: true, name: "b.mp4", url: "blob:gone", path: "/refs/b.mp4", poster: "data:image/jpeg;base64,AA" },
  ];
  video.status = "Sending…";
  const json = serialize();
  expect(serialize()).toBe(json);
  restore(null);
  expect(board.cards).toEqual([]);
  expect(serialize()).toBe("");

  const read = async (path: string) => new TextEncoder().encode(path).buffer as ArrayBuffer;
  globalThis.URL.createObjectURL ??= () => "blob:restored";
  restore(json, read);
  const [back] = board.cards;
  expect(back).toMatchObject({ kind: "video", text: "Remake" });
  expect(back.status).toBeUndefined();
  expect(back.refs[1]).toMatchObject({ poster: "data:image/jpeg;base64,AA", path: "/refs/b.mp4" });
  expect(back.refs[1].url).toBeUndefined();
  await new Promise((r) => setTimeout(r, 0));
  expect(back.refs[0].url).toMatch(/^blob:/);
  expect(addCard(null).id).toBeGreaterThan(back.id);
});

it("splits a styled script into spoken scenes", () => {
  expect(scenes("## 1. Hook · 0:00–0:08\nLearning to code?\n\n## 2. What it is\nGitHub Education helps.\nIt is free.")).toEqual([
    { scene: "1. Hook · 0:00–0:08", lines: "Learning to code?" },
    { scene: "2. What it is", lines: "GitHub Education helps.\nIt is free." },
  ]);
  expect(scenes("Just one script.")).toEqual([{ scene: "Script", lines: "Just one script." }]);
});

it("finds scenes from blank lines, merges storyboard batches and flags repeated components", () => {
  expect(scenes("One line.\nTwo.\n\nThird.").map((s) => s.lines)).toEqual(["One line.\nTwo.", "Third."]);
  board.cards = [];
  const video = addCard(null, { kind: "video", text: "x" });
  board.active = video.id;
  const svg = "<svg/>";
  openStoryboard({ beats: [1, 2, 3, 4].map((n) => ({ line: `L${n}`, svg, component: n === 2 ? "Step flow" : `C${n}` })) });
  openStoryboard({ from: 5, beats: [5, 6].map((n) => ({ line: `L${n}`, svg, component: n === 6 ? "step flow" : `C${n}` })) });
  const storyboard = board.cards.find((c) => c.kind === "storyboard")!;
  expect(storyboard.beats!.map((b) => b.line)).toEqual(["L1", "L2", "L3", "L4", "L5", "L6"]);
  expect(repeated(storyboard)).toEqual(["step flow"]);
  openStoryboard({ beats: [{ line: "Fresh", svg }] });
  expect(storyboard.beats!.map((b) => b.line)).toEqual(["Fresh"]);
  board.active = undefined;
});

it("flags a video whose length or scenes don't match its voiceover and storyboard", () => {
  // The placeholder build: old ten-second scenes against the real voice.
  expect(outOfSync({ duration: 118, scenes: 12 }, 96.24, 12)).toBe("The video is 118.0 s; the voiceover is 96.2 s.");
  // The style's 0.6 s tail on the last scene is fine.
  expect(outOfSync({ duration: 96.84, scenes: 12 }, 96.24, 12)).toBeUndefined();
  expect(outOfSync({ duration: 90, scenes: 11 }, 96.24, 12)).toBe("The video is 90.0 s; the voiceover is 96.2 s, and it has 11 scenes for 12 storyboard beats.");
  // Nothing to compare: no voice, no storyboard, or a project without sub-compositions.
  expect(outOfSync({ duration: 118, scenes: 0 }, undefined, 12)).toBeUndefined();
  expect(outOfSync({}, 96, 12)).toBeUndefined();
});
