// What the canvas asks the agent to do. One rule per line, so each rule is easy
// to find, change and test; rules used by several steps are written once here.

const lines = (...rules: (string | false | undefined)[]) => rules.filter(Boolean).join("\n");

/** Long builds go a few scenes at a time, so no single reply has to hold the whole film. */
export const STEPS = "- Work in steps: write or edit the generator a few scenes at a time, and rebuild and check between steps.";

/** How every build or change ends. */
export const PREVIEW = lines(
  "- Call video_preview with the project folder. The app starts the preview server; don't start one yourself.",
  "- Don't render until I ask.",
);

/** The rule that keeps fake audio out of a video. */
const REAL_VOICE = "- Use only this voiceover. Replace any silent or placeholder voice track with it.";

/** The style rule added to every step of a styled video. */
export const styleRule = (name: string, path: string) =>
  lines(
    `Style: follow the ${name} skill.`,
    `- Read ${path} and the references it names before starting.`,
    "- Its workflow, layout, components, motion, captions and sound rules come before any generic canvas instruction.",
    "- A script for it has one `## N. Scene name` heading per scene.",
    "- Every scene gets a different body from the style's component catalogue, and its lines feed that body: steps for a flow, two sides for a comparison, values for meters and charts. Label values as examples when they aren't sourced.",
    "- The voice is the one the user made on the script card in this session. Never pick or change it.",
  );

/** This session's voiceover: one clip per scene, or one clip to cut at the scene boundaries. */
export function voiceover(clips: { scene: string; path: string }[], whole?: string) {
  if (clips.length) {
    return lines(
      "Voiceover: one ElevenLabs clip per scene, made in this session with the voice the user picked.",
      ...clips.map((c, i) => `- scene ${i + 1} (${c.scene}): ${c.path}`),
      "Copy them into assets/vo/raw/ in order, then normalise and transcribe each.",
      REAL_VOICE,
    );
  }
  return lines(
    `Voiceover: one ElevenLabs clip for the whole script, made in this session with the voice the user picked: ${whole}`,
    "- Transcribe it and cut it into one clip per scene, from each scene's first word to its last.",
    "- Put the clips in assets/vo/raw/ in order, then normalise them.",
    REAL_VOICE,
  );
}

export const writeScript = (video: string, o: { again: boolean; styled: boolean; current?: string; frames: boolean }) =>
  lines(
    o.again ? "Write a new version of the script for this video. It replaces the current one." : "Write the script for this video.",
    o.styled &&
      "- Follow the style's script step: write SCRIPT.md in the project with its scenes, a different component for each scene and its fact sheet.",
    o.styled && "- Send text_to_speech only the spoken lines, under one `## N. Scene name` heading per scene. The headings aren't read aloud.",
    "- Send the script with text_to_speech, then stop for my review. Don't make the storyboard yet.",
    o.frames && "- Follow the reference frames.",
    `Video: ${video}`,
    o.current && `\nCurrent script:\n${o.current}`,
  );

export const makeStoryboard = (video: string, script: string | undefined, o: { styled: boolean; frames: boolean }) =>
  lines(
    "Make the storyboard for this confirmed script, then stop for my review. It replaces any storyboard so far.",
    o.styled && "- Make one beat per scene, not per sentence. A beat's `line` is the scene's spoken lines.",
    o.styled && "- Set each beat's `component` to the catalogue body planned for its scene. No two scenes share one.",
    o.styled && "- Draw each `svg` as the scene's final frame, exactly to the style's layout grid, colours and type, using that body. Use portrait 1080×1920 if the style says so.",
    o.styled && "- Send the beats in batches of up to 4 with `from`.",
    !o.styled && o.frames && "- Follow the style of the reference frames.",
    `Video: ${video}`,
    `\nScript:\n${script ?? "(no script yet)"}`,
  );

export const redoBeat = (n: number, line: string) =>
  `Redraw only beat ${n} ("${line}"). Call storyboard again with every beat in the same order, and keep the other beats' lines and SVGs exactly as they were.`;

export type Mode = "visuals" | "words" | "rebuild" | "voice";

/** A follow-up on a finished video. `voice` is required for "visuals" and "voice". */
export function followUp(mode: Mode, request: string, o: { styled: boolean; voice?: string }) {
  const ask = request.trim() && `Follow-up on the finished video: ${request.trim()}`;
  switch (mode) {
    case "words":
      return lines(
        ask,
        "This changes the words.",
        `- Send the updated script with text_to_speech${o.styled ? ", keeping its `## N. Scene name` headings, and update SCRIPT.md" : ""}.`,
        "- Call storyboard with the beats updated to match. Keep the SVGs of beats that didn't change.",
        "- Then stop. The app makes the new voiceover and asks you to rebuild the video.",
      );
    case "rebuild":
      return lines(
        ask,
        "Rebuild this video from the confirmed script, and keep the script exactly as it is.",
        `- First make a new storyboard${o.styled ? " with a different body from the style's catalogue for every scene, sent in batches of up to 4 with `from`" : ""}.`,
        "- Then stop. The app reuses this session's voiceover and asks you to build the video from the new storyboard.",
      );
    case "voice":
      return lines(
        "Put this session's voiceover into the video, which was built without it.",
        "- Re-time captions and reveals to its transcript, and keep the visuals.",
        STEPS,
        "",
        o.voice,
        "",
        "Update the HyperFrames project.",
        PREVIEW,
      );
    case "visuals":
      return lines(
        ask,
        "Change the visuals only, and keep the script.",
        o.styled && "- Scene layouts, components and motion may change within the style's rules.",
        STEPS,
        "",
        o.voice,
        "",
        "Update the HyperFrames project.",
        PREVIEW,
      );
  }
}

export const continueRun = (step: string) =>
  lines(
    `Your last run stopped before finishing, right after this step: ${step}.`,
    "- Check what that step did: its output and the files it changed. It may have failed even if the exit code was 0.",
    "- Fix it if needed, then finish the remaining work.",
  );

export const fixSync = (mismatch: string) =>
  lines(
    `The built video doesn't match. ${mismatch}`,
    "- Re-time every scene to its voice clip: the clip's length, plus the style's tail if it has one.",
    "- Keep one scene per storyboard beat.",
    "- Re-time captions and reveals to the transcript.",
    "- Check the project's total length after rebuilding.",
  );

/** Building an unstyled video from its storyboard SVGs. */
export const build = (beats: string[], o: { rebuild: boolean; voice?: string }) =>
  lines(
    `I confirmed this storyboard. ${o.rebuild ? "Rebuild the existing HyperFrames video from it." : "Build the video with HyperFrames."}`,
    "- Animate each beat's elements in as its line is spoken, not as a still picture, with transitions between beats.",
    PREVIEW,
    "",
    "Beats:",
    ...beats,
    "",
    o.voice ? `Voiceover: ${o.voice}` : "Voiceover: none yet. Time each beat to its line read at a natural pace, so it can be re-timed when the voiceover is made.",
  );

/** Building a styled episode with the style's own workflow. */
export const buildStyled = (style: string, plan: string[], o: { rebuild: boolean; voice: string }) =>
  lines(
    `I confirmed the storyboard. ${o.rebuild ? "Rebuild this episode" : "Build this episode"} with the ${style} skill's episode workflow, from its project step to its preview step.`,
    "- Use the confirmed scenes and SCRIPT.md, the skill's generator and its bundled example for each body's markup and motion.",
    "- Time reveals and captions to the words in the transcripts, and use the style's sound mix.",
    "- Don't use the storyboard SVGs as slides. They only show the intended layout.",
    STEPS,
    "- There's no music bed yet. Leave music out, with no silent file, and say so.",
    PREVIEW,
    "",
    "Build each scene with its planned body. Every scene uses a different one:",
    ...plan,
    "",
    o.voice,
  );

export const RENDER = "Render the final video now, then call video_preview with its file.";
export const SKIP = "Skip these questions. Use your best judgement and carry on.";
export const answers = (text: string) => `My answers:\n${text}\n\nCarry on from here.`;
