# Scene bodies

Every body goes inside `<div class="bd">` (top 262). Ids are `<scene>-<part>` and must be unique across the film. Working markup and motion for each one is in `examples/vercel-vs-vps.py` (in this skill), under `BODY["<id>"]` and `JS["<id>"]` for the scene id given.

| Body | When | Markup sketch | Motion | Example |
|---|---|---|---|---|
| **A vs B cards** | hook | `.cd.hi-tl` (A: `.m`, `.big`, `.s`) + `.vs` + `.cd` (B), body top 250 | `up` A, `fade` vs, `up` B, `pulse` each on its name | `hook` |
| **Step flow** | "how it works" | 3 `.cd` steps with `.ar` ↓ between them; the last one is `.hi-lm` | `up` each step, `fade` each arrow | `vercel` |
| **Layer stack** | what you set up yourself | `.cd.st` rows (`.t` name + `.m` tag), the bottom row `.dim`, revealed bottom-up | `up` from the bottom up, `pulse` the base | `vps` |
| **Who-does-what table** | responsibility split | `.th` header, then `.row` lines with `<span class=lm>auto</span> · <span class=tl>you</span>` | `up` per row, `pulse` the columns on the call-back | `who` |
| **Meter list** | plan quotas | `.mt` blocks: `.row.nb` (label + `.n` value) + `.bar > i` | `grow` to each level, the limit bar grows to 1 then `tint` red, then a `.cd.hi-rd` "Limit hit" | `hobby` |
| **Limits table** | per-plan numbers | `.th` "LIMIT · PLAN A · PLAN B", `.row` with `.n` values, a `.badge` error | `up` rows, `pop` badge + `sfx` | `timer` |
| **Usage track** | credit then overage | `.th` with a live `.rd` amount, `.track` (teal credit `i` + `.stripes` overage `i`), `.tk` ticks, two cards, `.fine` note | `grow` credit + `count`, `grow` overage `"none"` + second `count(first=false)` | `pro` |
| **Alert + choice** | what happens on a spike | number card with `.num` count, then `.two` (keep `.hi-lm` / pause `.hi-rd`), then a side-B card | `count`, `up`, `pulse` | `spike` |
| **Failure meters** | what breaks at the limit | `.cd` with `.st` (title + coloured `.n` tag), `.bar`, `.s` consequence | `up`, `grow`, `pop` tag + `sfx` | `vpslimit` |
| **Two numbered lists** | how to scale each | `.cd.hi-tl` and `.cd.hi-lm`, each with `.li` lines + `.s` note | `up` card, `fade` each line, `pulse` both | `more` |
| **Moving line chart** | cost vs traffic | SVG viewBox 396×230, axis, dashed guide, two paths, tip dots, labels, mono axis ticks + two cards | see motion.md "Charts must move" | `pay` |
| **Verdict cards** | the close | `.cd.hi-tl` "PICK A IF" + `.cd.hi-lm` "PICK B IF", 3 `.li` each | `up`, `fade`, then `pulse` **both together** | `which` |

## Typical episode order (12 scenes)

1. Hook
2. What A is
3. What B is
4. Who handles what
5. A's free-tier ceiling
6. A's per-request limits
7. A's paid ceiling
8. What happens on a spike
9. What happens when B hits its limit
10. How to get more on each
11. How you pay
12. Which one (neutral)

Change the order to fit the topic. Keep the hook first and the neutral verdict last.
