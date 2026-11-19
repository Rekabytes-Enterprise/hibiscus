# Sound

| Layer | File | Track | Volume | Timing |
|---|---|---|---|---|
| Voice | `assets/vo/NN-name.mp3` | 10 | 1 | scene start + 0.2s (`LEAD`), `data-duration` = clip length |
| Music | `assets/music/bed.mp3` | 11 | 0.14 | 0 → end, `data-fade-in="0.5"`, `data-fade-out="2.5"` |
| Whoosh | `assets/sfx/whoosh.mp3` / `whoosh-short.mp3`, alternating | 12 | 0.35 | 0.12s before every cut, 0.57s long |
| Pop / click | `assets/sfx/pop.mp3` / `click-soft.mp3`, alternating | 13 | 0.45 | at each `sfx(t)` marker + 0.02s, about 5-8 per film |

## Rules

- **Durations:** every `<audio>` has an `id` and a `data-duration`. Without one, the clip runs to the end of the film and the check flags `duplicate_audio_track`.
- **Voice loudness:** normalise each voice clip on its own and keep the original in `assets/vo/raw/`:
  `ffmpeg -i raw/NN.mp3 -af loudnorm=I=-16:TP=-1.5:LRA=11 -ar 44100 -b:a 192k NN.mp3`
- **Music:** an ElevenLabs music instrumental with `eleven_music_v2_5`. Prompt for a calm, modern, minimal tech bed with soft pulses, no vocals and no drops, 2-3s longer than the film. Raw it measures about -15.5 LUFS. At 0.14 it sits about 16 dB under the voice.
- **Pops:** put pops only on hits: a limit reached, an error badge, an overage landing, a failure tag. Don't put them on every card.
- **Final mix:** about -14 LUFS integrated, peak ≤ -1 dBFS. Measure it with:
  `ffmpeg -nostats -i out.mp4 -af ebur128=peak=true -f null - 2>&1 | grep -E "^\s+(I|Peak):"`
- **Listen:** measuring isn't listening. Tell the person which parts you haven't heard.
