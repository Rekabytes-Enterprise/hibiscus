---
name: tech-explainer-short
description: Make a vertical tech explainer short in the house style - dark navy cards, teal and lime accents, a fixed series header, word-synced reveals, a caption pill and a 12-segment progress bar, voiced with the ElevenLabs voice made for the episode and built with HyperFrames. Use for any new episode in this format (comparisons like "X vs Y", "how X works", "what limits X has"), and for keeping animations, layout, colours, captions and sound consistent across episodes.
hibiscus-style: true
---

# Tech explainer short

**Genre:** a vertical **kinetic data explainer**. It's an informative short, not a benchmark or a review: one narrator, one idea per scene, and every number or claim appears on screen as a card, bar, table or chart at the moment the voice says it. The first episode is "Hosting 101 · Vercel vs VPS". Its full generator is bundled as `examples/vercel-vs-vps.py`.

Load `/hyperframes` before building. This skill only adds the house style on top of it.

## Rules that make the series consistent

1. **Neutral.** Never promote either side. No "start on X", "X is better" or "just use X". Close with something like "Neither one wins. It depends on what you build." Give each side the same kind of card and the same emphasis (both cards glow together).
2. **Informative, not a benchmark.** No scores, winners or test results. Explain how each option works, where its limits are and what happens when you hit them.
3. **Every figure is sourced.** Keep a fact sheet in `SCRIPT.md` with the source and the date checked. Label made-up example numbers on screen ("Example amount, for illustration").
4. **Identical frame.** The kicker, rule, eyebrow, headline, body, caption pill and progress bar always sit in the same place, at the same size. See `references/style.md`. Don't make the header bigger.
5. **Show it as it's said.** Each reveal starts on the transcript time of the word that names it. See `references/motion.md`.
6. **Same voice and mix.** One voice for the whole episode, the ElevenLabs voice and model made for it (in Hibiscus, the one generated on the script card; never pick or change it), normalised to -16 LUFS, with music at 0.14, whooshes on cuts and pops on key hits. See `references/sound.md`.

## Episode workflow

1. **Script** in `SCRIPT.md`: 10-12 scenes of 6-18s, about 2-2:20 total, about 2.5 words a second. Each scene gets a label (eyebrow), a headline with one lime phrase, and a table of time, VO line and what's on screen. Add the fact sheet. Format and voice: `references/script-and-voice.md`.
2. **Project:** `npx hyperframes init <dir> --skip-skills --skill=hyperframes`. Rename `name` in `package.json` so it isn't "hyperframes", or npx breaks. Copy `template/build.py` from this skill to `<dir>/tools/build.py`, and copy the `sfx` set (whoosh, whoosh-short, pop, click-soft) from the media-use skill into `assets/sfx/`.
3. **Voice:** one ElevenLabs clip per scene, saved to `assets/vo/raw/`, then normalised into `assets/vo/`.
4. **Transcribe** each clip with `npx hyperframes transcribe assets/vo/<clip>.mp3`. It always writes `assets/vo/transcript.json`, so move each result to `assets/vo/transcripts/<clip>.json` before running the next one.
5. **Music:** ElevenLabs music (`eleven_music_v2_5`), an instrumental 2-3s longer than the film, saved to `assets/music/bed.mp3`.
6. **Build:** fill in `KICKER`, `SCENES`, `BODY` and `JS` in `tools/build.py`, taking the times from the transcripts. Pick scene bodies from `references/components.md` and copy their markup and JS from `examples/vercel-vs-vps.py`. Then run `python3 -I tools/build.py`.
7. **Check:** run `npx hyperframes check`, then `npx hyperframes snapshot --at ...` at the end of each scene and wherever you changed something. The accepted warnings are listed in `references/style.md`.
8. **Preview:** `npx hyperframes preview --background`. Stop there and wait for the person to approve.
9. **Render** after they approve: `npx hyperframes render -o renders/<slug>.mp4`. Expect about -14 LUFS integrated with peaks around -1 dBFS. Grab a few frames with ffmpeg to check them.

## Scene length

Scene length = VO clip length + 0.6s. The voice starts 0.2s in (`LEAD`), which leaves about 0.4s of tail before the cut. The generator works this out from `SCENES`. If a re-recorded clip comes out shorter, you can keep the old length, which keeps the film the same total duration. HyperFrames only warns that the audio slot is shortened.

## Files

- `examples/vercel-vs-vps.py`: the complete generator from episode 1, with all 12 scene bodies and the moving chart. Copy scenes from it.
- `template/build.py`: the generator. It writes `index.html`, `compositions/<scene>.html` and `compositions/captions.html`, holds the shared CSS and the motion helpers, and lays out the audio tracks.
- `references/style.md`: canvas, layout grid, colours, type and the component CSS classes.
- `references/motion.md`: the helper vocabulary, timings, intro and outro, sync rules and the moving line chart.
- `references/components.md`: the catalogue of scene bodies, with markup.
- `references/sound.md`: voice, music and SFX levels and processing.
- `references/script-and-voice.md`: the script format, writing rules, spoken spellings and the ElevenLabs settings.
