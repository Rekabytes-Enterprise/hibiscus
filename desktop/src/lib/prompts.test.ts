import { expect, it } from "vitest";
import * as prompts from "./prompts";

it("puts one rule on each line and attaches the real voiceover to every build", () => {
  const voice = prompts.voiceover([{ scene: "1. Hook", path: "/v/scene-01.mp3" }]);
  expect(voice).toContain("- scene 1 (1. Hook): /v/scene-01.mp3");
  expect(voice).toContain("Replace any silent or placeholder voice track");
  const whole = prompts.voiceover([], "/v/voiceover.mp3");
  expect(whole).toContain("for the whole script, made in this session with the voice the user picked: /v/voiceover.mp3");
  for (const mode of ["visuals", "voice"] as const) {
    const text = prompts.followUp(mode, "", { styled: true, voice });
    expect(text).toContain(voice);
    expect(text).toContain("the preview URL and the project folder");
    expect(text).not.toMatch(/^Follow-up/);
  }
  expect(prompts.buildStyled("tech", ["- scene 1: Step flow"], { rebuild: false, voice })).toContain("Build each scene with its planned body. Every scene uses a different one:\n- scene 1: Step flow");
  // Optional rules drop out instead of leaving blank lines.
  expect(prompts.followUp("words", "", { styled: false })).not.toContain("\n\n");
  expect(prompts.followUp("words", "shorter", { styled: false }).split("\n")[0]).toBe("Follow-up on the finished video: shorter");
});
