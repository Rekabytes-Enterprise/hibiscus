# Style

## Canvas

- The output is 1080×1920 at 30fps.
- Design everything on a **480×854 stage**: `.stage{width:480px;height:854px;transform:scale(2.25);transform-origin:0 0}`. Every number below is in stage pixels.
- Fonts: Inter for everything, and JetBrains Mono for labels, ticks and numbers.

## Layout grid (fixed, never changes between scenes)

| Element | Position | Type |
|---|---|---|
| Kicker `● SERIES · TOPIC` | top 66, left 42 | Mono 500 11px, tracking .18em, #8794ae, teal 6px dot |
| Rule | top 110, left/right 42, 1px | #5fe0c8 at 0.8 opacity |
| Eyebrow (section label) | top 128, left 42 | Mono 500 13px, tracking .2em, #5fe0c8, UPPERCASE |
| Headline | top 150, left/right 42 | Inter 800 38px, line-height 1.03, tracking -.035em, #e9eef6, one phrase in lime |
| Body | top 262, left/right 42 | cards, tables, charts |
| Caption pill | top 694, left/right 42, height 68 | rgba(14,24,42,.92), 1px #22314d border, radius 10. Text Inter 600 18px, centred |
| Progress bar | top 786, left/right 42 | 12 segments, gap 4, 3px tall, track #1d2a42, fill teal; each fills across its scene |

The kicker, rule and progress bar live in the root `#chrome` and stay on screen the whole time. The eyebrow, headline and body belong to each scene. Keep headlines to 2 lines and the body above y≈680 so it clears the pill.

## Colour

| Token | Hex | Use |
|---|---|---|
| bg | radial `#10294a` → `#0a1426` → `#070e1c` | background, plus one soft blue glow drifting slowly |
| text | #e9eef6 | headline, card titles |
| teal | #5fe0c8 | option A, rule, eyebrow, progress, arrows |
| lime | #c6f25a | headline accent, option B, "good" or "included" |
| red | #f07a6a | limits hit, errors (504, 503, 502), overage |
| grey label | #8794ae | mono labels, kicker, ticks, fine print |
| grey sub | #9aa6bb | card subtitles |
| card | rgba(20,32,56,.78), border #22314d, radius 10, padding 13px 16px | every card |
| divider | #1b2840 | table rows, bar tracks |

Option A is always teal and option B is always lime, in every scene. Red only means "limit or failure". It never stands for a side.

## Component classes (in `SCENE_CSS`)

- `.cd` is a card. Add `.hi-tl`, `.hi-lm` or `.hi-rd` for a coloured border, or `.dim` for a faded one.
- Text inside a card:
  - `.m` is a mono label.
  - `.t` is the title, 18px/600.
  - `.big` is a 26px/800 name.
  - `.s` is the grey subtitle. `.s.on` makes it white at 16px.
- Rows and tables:
  - `.st` is a flex row with the label on the left and the value on the right.
  - `.row` is a table row with a bottom divider. `.row.nb` drops the divider.
  - `.th` is a mono table header.
- Bars:
  - `.bar > i` is a 5px meter.
  - `.track > i` is an 18px stacked bar. `.stripes` gives the red overage fill.
  - `.tk` holds the tick labels under a track.
- Numbers and badges:
  - `.n` is a mono value.
  - `.num` is a big 30px number.
  - `.badge` is a red mono error badge.
- Layout and lists:
  - `.two` is a 2-column grid.
  - `.li` is a list line.
  - `.ar` is the teal ↓ arrow.
  - `.vs` is the grey "vs".
  - `.fine` is 12px fine print.

## Check warnings that are accepted

These come from matching the reference layout, so leave them:
- `outside_safe_zone` and `text_below_floor` on the kicker.
- `timeline_track_too_dense` on the captions.
- `line_unread` on a card that shows for about 2s while the caption says the same thing.

Fix every error.
