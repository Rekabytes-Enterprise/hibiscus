# Script and voice

## SCRIPT.md format

```
## N. Scene name · m:ss–m:ss

Label: `EYEBROW TEXT`
Headline: Plain words **lime words.**

| Time | VO / caption | On screen |
|---|---|---|
| 0:00 | One short spoken line. | What appears on that line |
```

End with a **fact sheet** listing each claim and its source URL, with the date checked. Add a section for general claims, such as how Linux behaves, that don't come from one vendor's docs.

## Writing the voiceover

- **Lines:** one idea per line and 3-10 words per line. Each line becomes one caption.
- **Speed:** about 2.5 words a second, so a scene runs 6-18s and an episode runs 2:00-2:20, or 300-340 words.
- **Wording:** use plain words a beginner knows. Name the consequence: "you get a 504", "Linux kills your app".
- **Headlines:** 3-7 words, with the payoff phrase in lime: "Free has a **ceiling.**"
- **Neutral:**
  - Describe both sides the same way.
  - No recommendations to start on or default to one side.
  - "Small app: A. Big, steady traffic: B." is fine because it's conditional.
  - The close says it depends.
- **No slop:** follow the `unslop` skill.

## Spoken spellings (in the TTS prompt only, never in captions)

- Nginx → "Engine X"
- 504 → "five oh four"
- Next.js → "Next JS"
- Write prices as "20 dollars" so they're read out properly.

Captions keep the real spelling, like "Nginx", "504", "$20" and "Next.js". Check each one in the Whisper transcript.

## ElevenLabs settings

- Voice and model: the ones made for the episode. In Hibiscus that's the voice and model generated on the script card; use those clips and never pick or change the voice. There is no default voice.
- Generations: one per scene (`generations_count: 1`), reusing one flow per episode.
- Get the file with `creative_get_generations`, then `curl` its `full_res_url` into `assets/vo/raw/NN-name.mp3`.
- To fix one line, regenerate the whole scene clip so the delivery stays even. Keep the old file as `*.old.mp3`.
- Credits: a scene is about 100-250 characters. A full episode is about 3.3k characters plus about 900 for music.
