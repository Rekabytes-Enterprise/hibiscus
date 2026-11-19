# Motion

The motion is calm and quick. Things slide up a little and settle, with no bounces except on error badges. Every timeline is paused, seekable and deterministic, following the HyperFrames contract.

## Helpers (defined in `HELPERS`, times are transcript times; `+L` is added for you)

| Helper | What it does | Default | Use for |
|---|---|---|---|
| `up(sel, t, d=0.38)` | opacity 0→1, y 14→0, `power3.out` | the default entrance | cards, rows, anything new |
| `fade(sel, t, d=0.3)` | opacity 0→1, `power1.out` | | list lines, labels, arrows, "vs" |
| `grow(sel, t, d=0.6, to=1, ease)` | scaleX 0→to from the left | `power2.inOut` | meters, stacked bars (`"none"` for a steady meter) |
| `pulse(sel, t)` | scale 1→1.04→1, 0.14s each way | | emphasis on the spoken word |
| `pop(sel, t)` | opacity 0→1, scale 0.6→1, `back.out(2.4)` | | error badges (504), status tags |
| `tint(sel, prop, colour, t)` | 0.2s colour change | | bar turning red at the limit |
| `count(sel, t, a, b, d, prefix, first=true)` | number ticks a→b | `power1.inOut` | money, counts. Pass `first=false` for a second count on the same element |
| `sfx(t)` | no-op marker | | the generator puts a pop sound at this time |

## Sync

- Every reveal starts on the **transcript time of the word that names it**. Read the times from `assets/vo/transcripts/<clip>.json`, not from the script estimates.
- Lists reveal one line per spoken item.
- Use a card's `pulse` for a call-back ("Vercel does them" pulses the "auto" cells).
- Give both sides the same treatment. If one card pulses on the verdict, the other pulses at the same time.

## Scene intro and outro (automatic)

- **Intro**, every scene except the first:
  - The eyebrow fades in over 0.25s at 0.02s.
  - The headline words come in from opacity 0, y 10 and blur 6px, over 0.4s with `power3.out`, staggered 0.05s apart, starting at 0.05s.
  - The first scene has no intro, so frame 0 already shows a headline.
- **Outro**, every scene except the last: the headline, eyebrow and body fade to 0 over 0.22s, starting 0.3s before the cut.
- **Cut:** a hard cut between scenes, covered by a whoosh 0.12s before it.
- **Background:** `#glow` drifts (-420, -260) over the whole film with `sine.inOut`.
- **Progress:** each segment's scaleX goes 0→1 across its scene with `ease:none`.
- **Captions:** one caption line per voice line, in the fixed pill. The pill itself never animates and stays on screen the whole film. Each line's text fades in over 0.16s from opacity 0 and y 4px, with `power2.out`, at its `data-start`. There's no exit animation: the next line replaces it. A line starts on the transcript time of its first word plus `LEAD`, and the first caption starts at 0 so the pill is never empty. Text is Inter 600 18px, centred, white.

## Charts must move

Line charts are never static polylines.

- **Shape:** the line is a smooth curve. Sample the function at about 48 points and join them with a Catmull-Rom spline converted to cubic Béziers.
- **Draw-on:** rebuild the path's `d` from a state object in `onUpdate`. Don't use a dashoffset draw on a fixed shape.
- **Make it feel alive:**
  - The "usage" line ripples gently all scene long: `ph` is tweened 0→9 with `ease:none` across the scene duration.
  - A dot rides the tip while the line draws.
  - After both lines are drawn, a dashed vertical traffic marker sweeps with `sine.inOut`, `yoyo:true`, `repeat:1`, and dots ride both lines with it.
- **Flat lines:** a flat "fixed price" line stays flat. It's the contrast.
- **Determinism:**
  - All of the motion comes from tweens on one state object, so it seeks cleanly.
  - Don't use `repeat:-1`, `Math.random()` or timers.
  - Give the first tween `immediateRender:true` and the later ones `immediateRender:false`.

Reference implementation: `JS["pay"]` in `examples/vercel-vs-vps.py` (in this skill). Copy its `curve()` and `render()` functions.

## Don'ts

- Don't add new eases, spins, 3D flips or bounces on cards.
- Don't animate the header or the progress bar beyond what's described here.
- Don't use CSS transforms on elements that GSAP moves. The `.stage` scale is set on the stage, which GSAP never touches.
- Don't use `from()`. Use `fromTo()`, so seeking backwards works.
