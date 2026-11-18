"""Generator for a tech-explainer-short video (see SKILL.md).

Copy into <project>/tools/build.py, then edit KICKER, SCENES, BODY and JS.
Every scene is designed on a 480x854 stage and scaled 2.25x to 1080x1920.
Reveal times in JS are transcript times of the word that names each part;
the helpers add LEAD, because each voice clip starts LEAD seconds into its scene.

Run: python3 -I tools/build.py   (then: npx hyperframes check)
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEAD = 0.2
W, H = 1080, 1920
KICKER = "SERIES NAME · EPISODE TOPIC"   # top line, uppercase, same on every scene

# (scene id, voice clip in assets/vo/ without .mp3, clip length s, eyebrow, headline with [lime] words,
#  captions [(text, transcript time of its first word)])
SCENES = [
    ("hook", "01-hook", 6.0, "THE QUESTION", "Option A or [option B?]", [
        ("First caption line.", 0.10), ("Second caption line.", 2.80)]),
    ("which", "02-which", 6.0, "SO WHICH ONE?", "Pick by [what you need.]", [
        ("Neither one wins.", 0.10), ("It depends on what you build.", 2.50)]),
]

BODY = {}
JS = {}

BODY["hook"] = """
<div class="bd" style="top:250px">
  <div id="hook-a" class="cd hi-tl"><div class="m">OPTION A</div><div class="big">Option A</div><div class="s">One-line description</div></div>
  <div id="hook-vs" class="vs">vs</div>
  <div id="hook-b" class="cd hi-lm"><div class="m">OPTION B</div><div class="big">Option B</div><div class="s">One-line description</div></div>
</div>"""
JS["hook"] = """up("#hook-a",0.10);fade("#hook-vs",1.4);up("#hook-b",2.80);"""

BODY["which"] = """
<div class="bd">
  <div id="which-a" class="cd hi-tl"><div class="m tl">PICK A IF</div><div id="which-a1" class="li">Reason one</div><div id="which-a2" class="li">Reason two</div></div>
  <div id="which-b" class="cd hi-lm"><div class="m lm">PICK B IF</div><div id="which-b1" class="li">Reason one</div><div id="which-b2" class="li">Reason two</div></div>
</div>"""
JS["which"] = """up("#which-a",0.10);fade("#which-a1",0.6);fade("#which-a2",1.0);up("#which-b",1.4);fade("#which-b1",1.8);fade("#which-b2",2.2);
pulse("#which-a",2.50);pulse("#which-b",2.50);"""

SCENE_CSS = """
#root{position:absolute;inset:0;font-family:Inter,sans-serif;color:#e9eef6}
.stage{position:absolute;left:0;top:0;width:480px;height:854px;transform:scale(2.25);transform-origin:0 0}
.ey{position:absolute;top:128px;left:42px;font:500 13px 'JetBrains Mono',monospace;letter-spacing:.2em;color:#5fe0c8}
.hd{position:absolute;top:150px;left:42px;right:42px;font-weight:800;font-size:38px;line-height:1.03;letter-spacing:-.035em;margin:0}
.hd .w{display:inline-block}
.lm{color:#c6f25a}.tl{color:#5fe0c8}.rd{color:#f07a6a}
.bd{position:absolute;top:262px;left:42px;right:42px}
.m{font-family:'JetBrains Mono',monospace;font-size:11px;letter-spacing:.14em;color:#8794ae}
.th{display:flex;justify-content:space-between;padding-bottom:4px}
.cd{background:rgba(20,32,56,.78);border:1px solid #22314d;border-radius:10px;padding:13px 16px;margin-top:11px}
.hi-tl{border-color:#5fe0c8}.hi-lm{border-color:#c6f25a}.hi-rd{border-color:#f07a6a}
.dim{border-color:#5b6884}.dim .t{color:#8a97ad}
.t{font-size:18px;font-weight:600}
.big{font-size:26px;font-weight:800;margin-top:4px;letter-spacing:-.02em}
.s{font-size:14px;color:#9aa6bb;margin-top:3px}.s.on{color:#e9eef6;font-size:16px}
.st{display:flex;justify-content:space-between;align-items:center}
.row{display:flex;justify-content:space-between;align-items:center;font-size:16px;font-weight:600;padding:11px 0;border-bottom:1px solid #1b2840}
.row.nb{border:0;padding:0}
.mt{margin-top:16px}.mt:first-child{margin-top:0}
.n{font-family:'JetBrains Mono',monospace;font-size:13px;font-weight:500}
.num{font-size:30px;font-weight:800}
.bar{height:5px;background:#1b2840;border-radius:3px;margin-top:8px;overflow:hidden}
.bar i{display:block;height:5px;width:100%;border-radius:3px;background:#5fe0c8;transform:scaleX(0);transform-origin:0 50%}
.track{display:flex;height:18px;margin-top:10px;border-radius:4px;overflow:hidden;background:#1b2840}
.track i{display:block;height:18px;transform:scaleX(0);transform-origin:0 50%}
.stripes{background:repeating-linear-gradient(135deg,#f07a6a 0 6px,#c45a4d 6px 12px)}
.tk{position:relative;display:flex;justify-content:space-between;font:500 11px 'JetBrains Mono',monospace;color:#8794ae;margin-top:6px}
.ar{text-align:center;color:#5fe0c8;font-size:18px;margin:4px 0}
.vs{text-align:center;font-weight:800;font-size:22px;color:#8794ae;margin:10px 0}
.two{display:grid;grid-template-columns:1fr 1fr;gap:10px}.two .cd{margin-top:11px}
.li{font-size:15px;line-height:1.55;margin-top:6px}
.badge{display:inline-block;font:500 11px 'JetBrains Mono',monospace;color:#0c1628;background:#f07a6a;border-radius:4px;padding:2px 6px;margin-left:6px;vertical-align:2px}
.fine{font-size:12px;color:#8794ae;margin-top:12px}
"""

HELPERS = """
const L=%s;
const up=(s,t,d=0.38)=>tl.fromTo(s,{opacity:0,y:14},{opacity:1,y:0,duration:d,ease:"power3.out"},t+L);
const fade=(s,t,d=0.3)=>tl.fromTo(s,{opacity:0},{opacity:1,duration:d,ease:"power1.out"},t+L);
const grow=(s,t,d=0.6,to=1,ease="power2.inOut")=>tl.fromTo(s,{scaleX:0},{scaleX:to,duration:d,ease:ease},t+L);
const pulse=(s,t)=>tl.fromTo(s,{scale:1},{scale:1.04,duration:0.14,ease:"power1.out",yoyo:true,repeat:1},t+L);
const pop=(s,t)=>tl.fromTo(s,{opacity:0,scale:0.6},{opacity:1,scale:1,duration:0.28,ease:"back.out(2.4)"},t+L);
const tint=(s,p,c,t)=>tl.to(s,{[p]:c,duration:0.2},t+L);
const draw=(s,t,d)=>tl.fromTo(s,{strokeDashoffset:1},{strokeDashoffset:0,duration:d,ease:"power2.inOut"},t+L);
const count=(s,t,a,b,d,pre,first=true)=>{const o={v:a};const el=document.querySelector(s);tl.fromTo(o,{v:a},{v:b,duration:d,ease:"power1.inOut",immediateRender:first,onUpdate:()=>{el.textContent=pre+Math.round(o.v)}},t+L);};
const sfx=()=>{};
""" % LEAD


def headline(text, sid):
    out, lime = [], False
    for tok in text.replace("[", " [ ").replace("]", " ] ").split():
        if tok == "[":
            lime = True
        elif tok == "]":
            lime = False
        else:
            out.append(f'<span class="w{" lm" if lime else ""}">{tok}</span>')
    return f'<p id="{sid}-hd" class="hd">' + " ".join(out) + "</p>"


def scene_html(i, sid, eyebrow, head, dur):
    first, last = i == 0, i == len(SCENES) - 1
    intro = "" if first else f'tl.fromTo("#{sid}-ey",{{opacity:0}},{{opacity:1,duration:0.25}},0.02);' \
        f'tl.fromTo("#{sid}-hd .w",{{opacity:0,y:10,filter:"blur(6px)"}},{{opacity:1,y:0,filter:"blur(0px)",duration:0.4,ease:"power3.out",stagger:0.05}},0.05);'
    outro = "" if last else f'tl.to(["#{sid}-hd","#{sid}-ey","#{sid}-body"],{{opacity:0,duration:0.22,ease:"power1.in"}},{dur - 0.3:.2f});'
    return f"""<template>
  <style>{SCENE_CSS}</style>
  <div id="root" data-composition-id="{sid}" data-width="{W}" data-height="{H}">
    <div id="{sid}-stage" class="stage">
      <div id="{sid}-ey" class="ey">{eyebrow}</div>
      {headline(head, sid)}
      <div id="{sid}-body">{BODY[sid]}</div>
    </div>
  </div>
  <script>
    const tl = gsap.timeline({{ paused: true }});
    {HELPERS}
    {intro}
    {JS[sid]}
    {outro}
    tl.set({{}}, {{}}, {dur:.2f});
    window.__timelines["{sid}"] = tl;
  </script>
</template>
"""


def main():
    starts, t = [], 0.0
    for s in SCENES:
        d = round(s[2] + 0.6, 1)
        starts.append((t, d))
        t = round(t + d, 1)
    total = t

    comp = ROOT / "compositions"
    comp.mkdir(exist_ok=True)
    for i, (sid, _vo, _l, ey, hd, _c) in enumerate(SCENES):
        (comp / f"{sid}.html").write_text(scene_html(i, sid, ey, hd, starts[i][1]))

    # captions: one group per voice line, absolute film time
    caps = []
    for i, s in enumerate(SCENES):
        st, d = starts[i]
        lines = s[5]
        for j, (text, ct) in enumerate(lines):
            a = 0 if (i == 0 and j == 0) else round(st + LEAD + ct, 2)
            b = round(st + LEAD + lines[j + 1][1], 2) if j + 1 < len(lines) else (
                round(starts[i + 1][0] + LEAD, 2) if i + 1 < len(SCENES) else total)
            caps.append((f"cap-{i + 1:02d}-{j + 1}", text, a, round(b - a, 2)))
    cap_html = "\n".join(
        f'      <div id="{cid}" class="clip cap" data-start="{a}" data-duration="{d}" data-track-index="0"><span>{txt}</span></div>'
        for cid, txt, a, d in caps)
    (comp / "captions.html").write_text(f"""<template>
  <style>
    #root{{position:absolute;inset:0;font-family:Inter,sans-serif}}
    .cstage{{position:absolute;left:0;top:0;width:480px;height:854px;transform:scale(2.25);transform-origin:0 0}}
    .pill{{position:absolute;left:42px;right:42px;top:694px;height:68px;background:rgba(14,24,42,.92);border:1px solid #22314d;border-radius:10px}}
    .cap{{position:absolute;left:42px;right:42px;top:694px;height:68px;display:flex;align-items:center;justify-content:center;text-align:center;padding:0 18px;color:#e9eef6;font-size:18px;font-weight:600;line-height:1.3}}
  </style>
  <div id="root" data-composition-id="captions" data-width="{W}" data-height="{H}">
    <div class="cstage" data-layout-allow-caption-zone>
      <div id="cap-pill" class="pill"></div>
{cap_html}
    </div>
  </div>
  <script>
    const tl = gsap.timeline({{ paused: true }});
    document.querySelectorAll("#root .cap span").forEach((el) => {{
      const host = el.parentElement;
      tl.fromTo(el, {{ opacity: 0, y: 4 }}, {{ opacity: 1, y: 0, duration: 0.16, ease: "power2.out" }}, parseFloat(host.dataset.start));
    }});
    tl.set({{}}, {{}}, {total});
    window.__timelines["captions"] = tl;
  </script>
</template>
""")

    hosts = "\n".join(
        f'      <div id="{s[0]}" class="clip" data-composition-id="{s[0]}" data-composition-src="compositions/{s[0]}.html" data-start="{st}" data-duration="{d}" data-track-index="1" data-track-kind="graphics" data-width="{W}" data-height="{H}" style="z-index:3"></div>'
        for s, (st, d) in zip(SCENES, starts))
    vo = "\n".join(
        f'      <audio id="vo-{i + 1:02d}" class="clip" src="assets/vo/{s[1]}.mp3" data-start="{round(st + LEAD, 2)}" data-duration="{s[2]}" data-track-index="10" data-volume="1"></audio>'
        for i, (s, (st, _d)) in enumerate(zip(SCENES, starts)))
    whoosh = "\n".join(
        f'      <audio id="sfx-cut-{i + 1:02d}" class="clip" src="assets/sfx/{"whoosh-short" if i % 2 else "whoosh"}.mp3" data-start="{round(st - 0.12, 2)}" data-duration="0.57" data-track-index="12" data-volume="0.35"></audio>'
        for i, (st, _d) in enumerate(starts) if i > 0)
    pops = []
    for i, sid in enumerate(s[0] for s in SCENES):
        st = starts[i][0]
        for part in JS[sid].split("sfx(")[1:]:
            tt = float(part.split(")")[0])
            pops.append(round(st + LEAD + tt + 0.02, 2))
    pop_html = "\n".join(
        f'      <audio id="sfx-pop-{k + 1:02d}" class="clip" src="assets/sfx/{"pop" if k % 2 == 0 else "click-soft"}.mp3" data-start="{t0}" data-duration="{0.72 if k % 2 == 0 else 0.36}" data-track-index="13" data-volume="0.45"></audio>'
        for k, t0 in enumerate(pops))
    music = ROOT / "assets/music/bed.mp3"
    music_html = (f'      <audio id="music-bed" class="clip" src="assets/music/bed.mp3" data-start="0" data-duration="{total}" '
                  f'data-track-index="11" data-volume="0.14" data-fade-in="0.5" data-fade-out="2.5"></audio>') if music.exists() else ""
    segs = "".join(f'<div class="seg"><i id="prog-{i + 1:02d}"></i></div>' for i in range(len(SCENES)))
    prog_js = "\n".join(
        f'      tl.fromTo("#prog-{i + 1:02d}",{{scaleX:0}},{{scaleX:1,duration:{d},ease:"none"}},{st});' for i, (st, d) in enumerate(starts))

    (ROOT / "index.html").write_text(f"""<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width={W}, height={H}" />
    <script src="https://cdn.jsdelivr.net/npm/gsap@3.15.0/dist/gsap.min.js"></script>
    <style>
      * {{ margin: 0; padding: 0; box-sizing: border-box; }}
      html, body {{ margin: 0; width: {W}px; height: {H}px; overflow: hidden; background: #070e1c; }}
      #bg {{ position: absolute; inset: 0; z-index: 0; background: radial-gradient(120% 60% at 70% 40%, #10294a 0%, #0a1426 60%, #070e1c 100%); }}
      #glow-wrap {{ position: absolute; inset: 0; z-index: 1; overflow: hidden; }}
      #glow {{ position: absolute; left: 380px; top: 520px; width: 900px; height: 900px; border-radius: 50%; background: radial-gradient(circle, rgba(46,120,190,.22) 0%, rgba(46,120,190,0) 65%); }}
      #chrome {{ position: absolute; left: 0; top: 0; width: 480px; height: 854px; transform: scale(2.25); transform-origin: 0 0; z-index: 2; }}
      .kk {{ position: absolute; top: 66px; left: 42px; font: 500 11px 'JetBrains Mono', monospace; letter-spacing: .18em; color: #8794ae; }}
      .kk b {{ display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: #5fe0c8; margin-right: 9px; vertical-align: 1px; }}
      .rl {{ position: absolute; top: 110px; left: 42px; right: 42px; height: 1px; background: #5fe0c8; opacity: .8; }}
      .pg {{ position: absolute; left: 42px; right: 42px; top: 786px; display: flex; gap: 4px; }}
      .seg {{ flex: 1; height: 3px; background: #1d2a42; border-radius: 2px; overflow: hidden; }}
      .seg i {{ display: block; height: 3px; width: 100%; background: #5fe0c8; transform: scaleX(0); transform-origin: 0 50%; }}
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="main" data-start="0" data-duration="{total}" data-width="{W}" data-height="{H}">
      <div id="bg"></div>
      <div id="glow-wrap" data-layout-ignore><div id="glow"></div></div>
      <div id="chrome">
        <div id="kicker" class="kk"><b></b>{KICKER}</div>
        <div id="rule" class="rl"></div>
        <div id="progress" class="pg" data-layout-allow-caption-zone>{segs}</div>
      </div>
{hosts}
      <div id="captions" class="clip" data-composition-id="captions" data-composition-src="compositions/captions.html" data-start="0" data-duration="{total}" data-track-index="2" data-track-kind="captions" data-width="{W}" data-height="{H}" style="z-index:4"></div>
{vo}
{music_html}
{whoosh}
{pop_html}
    </div>
    <script>
      const tl = gsap.timeline({{ paused: true }});
      tl.fromTo("#glow", {{ x: 0, y: 0 }}, {{ x: -420, y: -260, duration: {total}, ease: "sine.inOut" }}, 0);
{prog_js}
      window.__timelines["main"] = tl;
    </script>
  </body>
</html>
""")
    print(f"{len(SCENES)} scenes, {len(caps)} captions, {len(pops)} pops, total {total}s, music={'yes' if music.exists() else 'no'}")


if __name__ == "__main__":
    main()
