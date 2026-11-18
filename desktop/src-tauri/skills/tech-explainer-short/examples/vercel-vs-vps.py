"""Generate index.html and compositions/*.html for the Vercel vs VPS video.

Every scene is designed on a 480x854 stage (the reference video's size) and
scaled 2.25x to the 1080x1920 canvas. Reveal times are the transcript times of
the word that names each part; LEAD shifts them to match the voice clip, which
starts LEAD seconds into its scene.

Run: python3 tools/build.py
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEAD = 0.2
W, H = 1080, 1920

# (scene id, voice clip, clip length, eyebrow, headline with [lime] words, captions)
SCENES = [
    ("hook", "01-hook", 6.3, "WHERE DOES YOUR APP LIVE?", "Vercel or a [VPS?]", [
        ("You built an app.", 0.11), ("Now where does it live?", 1.40),
        ("Vercel, or a VPS?", 3.15), ("Let's compare.", 5.36)]),
    ("vercel", "02-vercel", 7.8, "OPTION A", "Vercel: [push] and it's live.", [
        ("With Vercel, you push your code.", 0.03), ("It builds the app for you,", 2.41),
        ("and puts it online with HTTPS.", 3.88), ("No server to touch.", 6.45)]),
    ("vps", "03-vps", 9.8, "OPTION B", "A VPS is a [rented computer.]", [
        ("A VPS is a rented computer.", 0.04), ("The provider gives you the machine.", 2.69),
        ("You install Linux, Nginx, SSL, and the database.", 4.68), ("It's all yours.", 8.64)]),
    ("who", "04-who-handles", 12.2, "WHO HANDLES WHAT", "The real difference is [the work.]", [
        ("The real difference is the work.", 0.06), ("Deploys, SSL, scaling, updates:", 2.45),
        ("Vercel does them. On a VPS, you do.", 6.43), ("But you get full control.", 9.94)]),
    ("hobby", "05-hobby-ceiling", 9.7, "HOBBY PLAN · PER MONTH", "Free has a [ceiling.]", [
        ("Vercel's free plan has monthly limits.", 0.04), ("The one you hit first is CPU:", 2.92),
        ("4 hours a month.", 5.20), ("Go over, and it pauses until the month resets.", 6.75)]),
    ("timer", "06-function-timer", 10.5, "PER FUNCTION LIMITS", "Every function has [a timer.]", [
        ("Every function also has a timer.", 0.03), ("5 minutes on Hobby, about 13 on Pro.", 2.51),
        ("Run too long, and you get a 504.", 5.99), ("A VPS has no timer.", 8.74)]),
    ("pro", "07-pro-ceiling", 9.0, "PRO PLAN · $20 / MONTH", "Pro has a ceiling [too.]", [
        ("Pro is $20 a month,", 0.15), ("with $20 of usage included.", 2.37),
        ("Go past that, and you're not stopped.", 4.44), ("You're billed for every extra bit.", 7.03)]),
    ("spike", "08-pay-or-offline", 11.0, "WHEN TRAFFIC SPIKES", "Pay more, or [go offline.]", [
        ("Say a post goes viral.", 0.06), ("Vercel emails you at $200.", 2.02),
        ("Then you choose: keep paying, or pause the site.", 4.76), ("A VPS just slows down instead.", 8.62)]),
    ("vpslimit", "09-vps-limit", 15.9, "WHEN A VPS HITS ITS LIMIT", "A VPS doesn't bill you. [It struggles.]", [
        ("So what happens when a VPS hits its limit?", 0.03), ("Max out the CPU, and everything slows down.", 3.15),
        ("Run out of RAM, and Linux kills your app.", 6.69), ("Fill the disk, and your database stops writing.", 9.63),
        ("Use up the bandwidth, and you pay extra or get slowed.", 12.63)]),
    ("more", "10-get-more", 18.0, "HOW TO GET MORE", "Raising the limit [works differently.]", [
        ("Need more? On Vercel, it scales by itself.", 0.04), ("Upgrade to Pro, or raise memory, run time, and budget.", 4.24),
        ("On a VPS, you resize to a bigger plan,", 8.04), ("add caching, or add a second server.", 10.64),
        ("Vercel, you change a setting.", 13.56), ("VPS, you change the server.", 15.84)]),
    ("pay", "11-how-you-pay", 9.7, "HOW YOU PAY", "One bill [grows.] One stays flat.", [
        ("Vercel charges for what you use.", 0.03), ("A VPS charges a flat price.", 2.47),
        ("Small app: Vercel.", 5.19), ("Big, steady traffic: VPS.", 7.15)]),
    ("which", "12-which-one", 10.9, "SO WHICH ONE?", "Pick by [what you need.]", [
        ("Pick Vercel for Next.js and zero server work.", 0.06), ("Pick a VPS for bots, long jobs, and steady traffic.", 3.70),
        ("Neither one wins.", 7.78), ("It depends on what you build.", 9.32)]),
]

# Scene bodies (HTML on the 480 stage) and reveal scripts. In the JS,
# times are transcript times; the helpers add LEAD.
BODY = {}
JS = {}

BODY["hook"] = """
<div class="bd" style="top:250px">
  <div id="hook-a" class="cd hi-tl"><div class="m">OPTION A · PLATFORM</div><div class="big">Vercel</div><div class="s">Hosting is done for you</div></div>
  <div id="hook-vs" class="vs">vs</div>
  <div id="hook-b" class="cd"><div class="m">OPTION B · SERVER</div><div class="big">VPS</div><div class="s">A server you rent and run</div></div>
</div>"""
JS["hook"] = """up("#hook-a",1.40);fade("#hook-vs",3.15);up("#hook-b",4.44);pulse("#hook-a",3.15);pulse("#hook-b",4.44);"""

BODY["vercel"] = """
<div class="bd">
  <div id="vercel-1" class="cd"><div class="m">STEP 1</div><div class="t">git push</div></div>
  <div id="vercel-a1" class="ar">↓</div>
  <div id="vercel-2" class="cd"><div class="m">STEP 2</div><div class="t">Vercel builds your app</div></div>
  <div id="vercel-a2" class="ar">↓</div>
  <div id="vercel-3" class="cd hi-lm"><div class="m">STEP 3</div><div class="t lm">Live with HTTPS and a URL</div><div class="s">served from Vercel's CDN</div></div>
</div>"""
JS["vercel"] = """up("#vercel-1",1.31);fade("#vercel-a1",2.41);up("#vercel-2",2.52);fade("#vercel-a2",4.05);up("#vercel-3",4.77);pulse("#vercel-3",6.45);"""

VPS_ROWS = [("app", "Your app", "YOU", 8.64), ("db", "Database", "YOU", 7.88), ("ng", "Nginx + SSL", "YOU", 5.87),
            ("node", "Node, Docker", "YOU", 5.62), ("linux", "Linux (Ubuntu)", "YOU", 5.37), ("hw", "Hardware", "PROVIDER", 2.82)]
BODY["vps"] = '<div class="bd"><div id="vps-lbl" class="m" style="margin-bottom:4px">WHAT YOU SET UP YOURSELF</div>' + "".join(
    f'<div id="vps-{k}" class="cd st{" dim" if k == "hw" else ""}" style="margin-top:{8 if i == 0 else 6}px"><span class="t">{t}</span><span class="m {"tl" if tag == "YOU" else ""}">{tag}</span></div>'
    for i, (k, t, tag, _) in enumerate(VPS_ROWS)) + "</div>"
JS["vps"] = 'fade("#vps-lbl",4.68);' + "".join(f'up("#vps-{k}",{t});' for k, _, _, t in VPS_ROWS) + 'pulse("#vps-hw",3.98);'

WHO_ROWS = [("dep", "Deploys", 2.45), ("ssl", "HTTPS / SSL", 3.78), ("scale", "Scaling", 4.85), ("upd", "Server updates", 5.52)]
BODY["who"] = ('<div class="bd"><div id="who-h" class="m th"><span>TASK</span><span>VERCEL · VPS</span></div>' + "".join(
    f'<div id="who-{k}" class="row"><span>{t}</span><span><span class="lm auto">auto</span> · <span class="tl you">you</span></span></div>'
    for k, t, _ in WHO_ROWS) +
    '<div id="who-full" class="row"><span>Full server access</span><span><span class="tl">no</span> · <span id="who-yes" class="lm">yes</span></span></div></div>')
JS["who"] = 'fade("#who-h",0.6);' + "".join(f'up("#who-{k}",{t});' for k, _, t in WHO_ROWS) + \
    'pulse("#who-stage .auto",6.43);pulse("#who-stage .you",8.87);up("#who-full",9.94);pulse("#who-yes",11.48);'

HOBBY = [("cpu", "Active CPU", "4 CPU-hrs", 1.0), ("inv", "Function invocations", "1M", 0.62), ("fdt", "Fast Data Transfer", "100 GB", 0.48),
         ("cdn", "CDN requests", "1M", 0.55), ("img", "Image transformations", "5,000", 0.30)]
BODY["hobby"] = '<div class="bd">' + "".join(
    f'<div id="hobby-{k}" class="mt"><div class="row nb"><span>{t}</span><span id="hobby-{k}-v" class="n">{v}</span></div><div class="bar"><i id="hobby-{k}-f"></i></div></div>'
    for k, t, v, _ in HOBBY) + \
    '<div id="hobby-hit" class="cd hi-rd st" style="margin-top:20px"><span class="t rd">Limit hit</span><span class="n">paused up to 30 days</span></div></div>'
JS["hobby"] = "".join(f'up("#hobby-{k}",{0.3 + i * 0.12:.2f});' for i, (k, *_r) in enumerate(HOBBY)) + "".join(
    f'grow("#hobby-{k}-f",1.52,0.9,{lv});' for k, _, _, lv in HOBBY[1:]) + \
    'grow("#hobby-cpu-f",2.92,2.2,1);tint("#hobby-cpu-f","background-color","#f07a6a",5.20);tint("#hobby-cpu-v","color","#f07a6a",5.20);' \
    'pulse("#hobby-cpu",5.20);up("#hobby-hit",7.50);sfx(5.20);'

TIMER = [("run", "Max run time", "5 min · <span class='lm'>13 min</span>", 2.51), ("mem", "Memory", "2 GB · <span class='lm'>4 GB</span>", 3.38),
         ("cpu", "CPU", "1 vCPU · <span class='lm'>2 vCPU</span>", 3.86), ("body", "Request body", "4.5 MB · 4.5 MB", 4.41),
         ("bundle", "Bundle size", "250 MB · 250 MB", 5.07)]
BODY["timer"] = ('<div class="bd"><div id="timer-h" class="m th"><span>LIMIT</span><span>HOBBY · PRO</span></div>' + "".join(
    f'<div id="timer-{k}" class="row"><span>{t}{" <span id=timer-504 class=badge>504</span>" if k == "run" else ""}</span><span class="n">{v}</span></div>'
    for k, t, v, _ in TIMER) +
    '<div id="timer-vps" class="cd hi-lm" style="margin-top:18px"><div class="m lm">ON A VPS</div><div class="s on">No timer. All your cores. All your RAM.</div></div></div>')
JS["timer"] = 'fade("#timer-h",0.6);' + "".join(f'up("#timer-{k}",{t});' for k, _, _, t in TIMER) + \
    'pop("#timer-504",7.61);sfx(7.61);up("#timer-vps",8.74);'

BODY["pro"] = """
<div class="bd">
  <div id="pro-lbl" class="m th"><span>THIS MONTH'S USAGE</span><span id="pro-amt" class="rd">$0</span></div>
  <div class="track"><i id="pro-cr" style="width:13.5%;background:#5fe0c8"></i><i id="pro-od" class="stripes" style="width:86.5%"></i></div>
  <div class="tk"><span>$0</span><span id="pro-mark" style="position:absolute;left:13.5%">$20 CREDIT</span><span>DAY 18</span></div>
  <div id="pro-inc" class="cd" style="margin-top:22px"><div class="m tl">INCLUDED</div><div class="t">$20 credit, any resource</div><div class="s">CPU, bandwidth, requests, images</div></div>
  <div id="pro-after" class="cd hi-rd"><div class="m rd">AFTER THAT</div><div class="t">On-demand billing</div><div class="s">Your app keeps running. The meter keeps going.</div></div>
  <div id="pro-note" class="fine">Example amount, for illustration</div>
</div>"""
JS["pro"] = """fade("#pro-lbl",0.15);grow("#pro-cr",2.37,1.3,1);count("#pro-amt",2.37,0,20,1.3,"$");fade("#pro-mark",2.51);up("#pro-inc",2.51);
grow("#pro-od",4.44,4.1,1,"none");count("#pro-amt",4.44,20,148,4.1,"$",false);up("#pro-after",5.85);fade("#pro-note",7.03);sfx(8.58);"""

BODY["spike"] = """
<div class="bd">
  <div id="spike-def" class="cd"><div class="m">DEFAULT ALERT</div><div class="st"><span class="t">Spend alert at</span><span id="spike-200" class="num lm">$0</span></div><div class="s">An email, not a stop</div></div>
  <div id="spike-bud" class="cd"><div class="m">IF YOU SET A BUDGET</div><div class="t">Alerts at 50%, 75%, 100%</div><div class="s">Checked every few minutes</div></div>
  <div class="two">
    <div id="spike-keep" class="cd hi-lm"><div class="m lm">KEEP GOING</div><div class="t">Bill grows</div></div>
    <div id="spike-pause" class="cd hi-rd"><div class="m rd">PAUSE</div><div class="t">Site shows 503</div></div>
  </div>
  <div id="spike-vps" class="cd hi-tl"><div class="m tl">ON A VPS</div><div class="s on">Same price. The site slows down instead.</div></div>
</div>"""
JS["spike"] = """up("#spike-def",2.02);count("#spike-200",2.02,0,200,1.6,"$");up("#spike-bud",4.10);up("#spike-keep",5.80);up("#spike-pause",6.95);pulse("#spike-pause",7.81);up("#spike-vps",8.62);"""

VL = [("cpu", "CPU 100%", "SLOW", "Every page loads slower", 3.15, 4.20, 5.64, "#f07a6a"),
      ("ram", "RAM full", "CRASH", "Linux kills your app → 502", 6.69, 7.33, 8.26, "#f07a6a"),
      ("disk", "Disk full", "STUCK", "Database and logs stop writing", 9.63, 10.10, 11.45, "#f07a6a"),
      ("bw", "Bandwidth used up", "FEE", "Extra fee or slowed, by provider", 12.63, 13.13, 14.22, "#c6f25a")]
BODY["vpslimit"] = '<div class="bd">' + "".join(
    f'<div id="vl-{k}" class="cd"><div class="st"><span class="t">{t}</span><span id="vl-{k}-tag" class="n" style="color:{c}">{tag}</span></div><div class="bar"><i id="vl-{k}-f" style="background:{c}"></i></div><div class="s">{s}</div></div>'
    for k, t, tag, s, _a, _b, _c, c in VL) + "</div>"
JS["vpslimit"] = "".join(f'up("#vl-{k}",{a});grow("#vl-{k}-f",{a + 0.2:.2f},{b - a:.2f},1);pop("#vl-{k}-tag",{c});sfx({c});'
                         for k, _t, _tag, _s, a, b, c, _col in VL)

BODY["more"] = """
<div class="bd">
  <div id="more-v" class="cd hi-tl"><div class="m tl">VERCEL</div>
    <div id="more-v1" class="li">1. Scales by itself</div><div id="more-v2" class="li">2. Upgrade Hobby → Pro</div>
    <div id="more-v3" class="li">3. Raise memory and max duration</div><div id="more-v4" class="li">4. Raise your spend budget</div>
    <div id="more-vn" class="s">Settings change. No downtime.</div></div>
  <div id="more-p" class="cd hi-lm"><div class="m lm">VPS</div>
    <div id="more-p1" class="li">1. Resize to a bigger plan</div><div id="more-p2" class="li">2. Add caching and a CDN</div>
    <div id="more-p3" class="li">3. Add a second server</div><div id="more-p4" class="li">4. Put a load balancer in front</div>
    <div id="more-pn" class="s">Resizing usually means a reboot.</div></div>
</div>"""
JS["more"] = """up("#more-v",1.16);fade("#more-v1",2.37);fade("#more-v2",4.24);fade("#more-v3",5.40);fade("#more-v4",7.28);fade("#more-vn",7.60);
up("#more-p",8.04);fade("#more-p1",9.04);fade("#more-p2",10.64);fade("#more-p3",11.90);fade("#more-p4",12.64);fade("#more-pn",12.90);
pulse("#more-v",13.56);pulse("#more-p",15.84);"""

BODY["pay"] = """
<div class="bd">
  <svg id="pay-chart" viewBox="0 0 396 230" style="width:100%;overflow:visible">
    <line id="pay-axis" x1="0" y1="210" x2="396" y2="210" stroke="#22314d" stroke-width="2"/>
    <line id="pay-guide" x1="0" y1="20" x2="0" y2="210" stroke="#5b6884" stroke-width="1.5" stroke-dasharray="4 5" opacity="0"/>
    <path id="pay-v" d="M0 205" fill="none" stroke="#5fe0c8" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
    <path id="pay-p" d="M0 165" fill="none" stroke="#c6f25a" stroke-width="4" stroke-linecap="round"/>
    <circle id="pay-vd" cx="0" cy="205" r="6" fill="#5fe0c8" opacity="0"/>
    <circle id="pay-pd" cx="0" cy="165" r="6" fill="#c6f25a" opacity="0"/>
    <text id="pay-vl" x="318" y="22" fill="#5fe0c8" font-size="16" font-weight="700" font-family="Inter">Vercel</text>
    <text id="pay-pl" x="340" y="155" fill="#c6f25a" font-size="16" font-weight="700" font-family="Inter">VPS</text>
    <text x="0" y="230" fill="#5b6884" font-size="12" font-family="JetBrains Mono">LOW TRAFFIC</text>
    <text x="300" y="230" fill="#5b6884" font-size="12" font-family="JetBrains Mono">HIGH TRAFFIC</text>
  </svg>
  <div id="pay-c1" class="cd"><div class="t">Vercel Pro</div><div class="s">$20 per developer / month, plus usage</div></div>
  <div id="pay-c2" class="cd"><div class="t">VPS</div><div class="s">Fixed monthly price for the server</div></div>
</div>"""
# The Vercel line is a smooth curve (Catmull-Rom through 48 samples) that rises with
# traffic and ripples gently; the VPS line stays flat. Both draw left to right with a
# dot on the tip, then a traffic marker sweeps along both lines. Everything is a
# function of timeline time, so it is seek-safe.
JS["pay"] = """const W=380,N=48,FLAT=165;
const st={pv:0,pp:0,ph:0,sw:0,g:0};
const vEl=document.querySelector("#pay-v"),pEl=document.querySelector("#pay-p");
const vd=document.querySelector("#pay-vd"),pd=document.querySelector("#pay-pd"),gd=document.querySelector("#pay-guide");
const vy=(x)=>{const u=x/W;return 205-172*Math.pow(u,1.7)-7*Math.sin(u*11-st.ph)*Math.min(1,u*3);};
const curve=(f,end)=>{if(end<=0)return "M0 "+f(0).toFixed(1);const pts=[];for(let i=0;i<=N;i++){const x=end*i/N;pts.push([x,f(x)]);}
  let d="M"+pts[0][0].toFixed(1)+" "+pts[0][1].toFixed(1);
  for(let i=0;i<pts.length-1;i++){const p0=pts[Math.max(0,i-1)],p1=pts[i],p2=pts[i+1],p3=pts[Math.min(pts.length-1,i+2)];
    d+=" C"+(p1[0]+(p2[0]-p0[0])/6).toFixed(1)+" "+(p1[1]+(p2[1]-p0[1])/6).toFixed(1)+" "+(p2[0]-(p3[0]-p1[0])/6).toFixed(1)+" "+(p2[1]-(p3[1]-p1[1])/6).toFixed(1)+" "+p2[0].toFixed(1)+" "+p2[1].toFixed(1);}
  return d;};
const render=()=>{const ve=W*st.pv,pe=W*st.pp;
  vEl.setAttribute("d",curve(vy,ve));pEl.setAttribute("d",pe>0?"M0 "+FLAT+" L"+pe.toFixed(1)+" "+FLAT:"M0 "+FLAT);
  const sx=st.sw>0?W*st.sw:ve;vd.setAttribute("cx",sx.toFixed(1));vd.setAttribute("cy",vy(sx).toFixed(1));
  const px=st.sw>0?W*st.sw:pe;pd.setAttribute("cx",px.toFixed(1));
  gd.setAttribute("x1",(W*st.sw).toFixed(1));gd.setAttribute("x2",(W*st.sw).toFixed(1));gd.setAttribute("opacity",st.g.toFixed(2));};
tl.fromTo(st,{ph:0},{ph:9,duration:10.3,ease:"none",immediateRender:true,onUpdate:render},0);
tl.fromTo(st,{pv:0},{pv:1,duration:1.6,ease:"power2.inOut",immediateRender:false,onUpdate:render},0.56+L);
tl.fromTo("#pay-vd",{opacity:0},{opacity:1,duration:0.2},0.56+L);
fade("#pay-vl",1.9);
tl.fromTo(st,{pp:0},{pp:1,duration:1.3,ease:"power2.inOut",immediateRender:false,onUpdate:render},2.88+L);
tl.fromTo("#pay-pd",{opacity:0},{opacity:1,duration:0.2},2.88+L);
fade("#pay-pl",3.9);
tl.fromTo(st,{g:0},{g:0.8,duration:0.3,immediateRender:false,onUpdate:render},4.5+L);
tl.fromTo(st,{sw:0},{sw:1,duration:0.01,immediateRender:false,onUpdate:render},4.5+L);
tl.fromTo(st,{sw:1},{sw:0.12,duration:2.4,ease:"sine.inOut",yoyo:true,repeat:1,immediateRender:false,onUpdate:render},4.51+L);
up("#pay-c1",5.19);up("#pay-c2",7.15);"""

BODY["which"] = """
<div class="bd">
  <div id="which-v" class="cd hi-tl"><div class="m tl">PICK VERCEL IF</div>
    <div id="which-v1" class="li">You build with Next.js</div><div id="which-v2" class="li">You want zero server work</div><div id="which-v3" class="li">Requests finish in seconds</div></div>
  <div id="which-p" class="cd hi-lm"><div class="m lm">PICK A VPS IF</div>
    <div id="which-p1" class="li">You run bots, workers, long jobs</div><div id="which-p2" class="li">You need full CPU and RAM</div><div id="which-p3" class="li">Traffic is high and steady</div></div>
</div>"""
JS["which"] = """up("#which-v",0.06);fade("#which-v1",1.20);fade("#which-v2",2.30);fade("#which-v3",3.00);
up("#which-p",3.70);fade("#which-p1",4.74);fade("#which-p2",5.68);fade("#which-p3",6.39);pulse("#which-v",7.78);pulse("#which-p",7.78);pulse("#which-v",9.32);pulse("#which-p",9.32);"""

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
        <div id="kicker" class="kk"><b></b>HOSTING 101 · VERCEL VS VPS</div>
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
