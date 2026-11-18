<script lang="ts">
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";
  import Models from "./Models.svelte";
  import { onMount } from "svelte";
  import { elevenlabs, saveReference, videoEngine, type Style, type Voice, type VoiceModel } from "./engine";

  import { addCard, busy, repeated, scenes, scriptOf, stale, styleOf, voiced, widthOf, board, removeCard, type Card, type Kind, type Model, type Reference } from "./board.svelte";

  /** The chat's model, which new cards start with. */
  let {
    model,
    reasoning,
    onconnect,
    onsend,
    canSend,
    onwrite,
    onredoscript,
    onconfirmscript,
    onredostoryboard,
    onredobeat,
    onfollowup,
    oncontinue,
    onfixsync,
    styles,
    onskip,
    onstop,
    onconfirm,
    onrender,
    onopenvideo,
    onanswer,
  }: {
    model: Model | null;
    reasoning: { thinking: string; levels: string[] };
    onconnect: () => void;
    /** Runs the agent on the card; `canSend` is false with no folder open. */
    onsend: (card: Card) => void;
    canSend: boolean;
    /** Video steps: draw the storyboard, confirm it into a video, render it, open the file. */
    onwrite: (video: Card) => void;
    onredoscript: (script: Card) => void;
    onconfirmscript: (script: Card) => void;
    onredostoryboard: (storyboard: Card) => void;
    onredobeat: (storyboard: Card, beat: number) => void;
    /** A follow-up on a finished video, by mode; "voice" adds the session's voiceover to a video built without it. */
    onfollowup: (preview: Card, request: string, mode: "visuals" | "words" | "rebuild" | "voice") => void;
    /** Picks up a run that stopped after a tool result without replying. */
    oncontinue: (card: Card) => void;
    /** Asks the agent to re-time a video that doesn't match its voiceover or storyboard. */
    onfixsync: (preview: Card) => void;
    /** Video styles to pick from on a Video card. */
    styles: Style[];
    onskip: (question: Card) => void;
    onstop: () => void;
    onconfirm: (card: Card) => void;
    onrender: (card: Card) => void;
    onopenvideo: (path: string) => void;
    /** Sends a Questions card's answers, then it closes. */
    onanswer: (card: Card) => void;
  } = $props();

  // The video engine: New appears once it is installed; until then, Download engine.
  let engine = $state<{ installed: boolean | null; step?: string; percent?: number | null; error?: string }>({ installed: null });
  onMount(() => {
    void videoEngine.installed().then((installed) => (engine.installed = installed)).catch(() => (engine.installed = false));
  });
  async function installEngine() {
    engine = { installed: false, step: "Starting…" };
    const stop = await videoEngine.onProgress((p) => {
      engine.step = p.step;
      engine.percent = p.percent;
    });
    try {
      await videoEngine.install();
      engine = { installed: true };
    } catch (e) {
      engine = { installed: false, error: String(e instanceof Error ? e.message : e) };
    } finally {
      stop();
    }
  }

  // ElevenLabs for Text to speech cards, loaded once the canvas opens.
  let tts = $state<{ connected: boolean; models: VoiceModel[]; voices: Voice[]; error?: string }>({ connected: false, models: [], voices: [] });
  onMount(async () => {
    tts.connected = !!(await elevenlabs.status().catch(() => null));
    if (!tts.connected) return;
    try {
      [tts.models, tts.voices] = await Promise.all([elevenlabs.models(), elevenlabs.voices()]);
    } catch (e) {
      tts.error = String(e instanceof Error ? e.message : e);
    }
  });
  // The documented default model, else the first one listed.
  const ttsModel = (card: Card) => card.speech?.model ?? (tts.models.find((m) => m.id === "eleven_multilingual_v2") ?? tts.models[0])?.id;
  const ttsVoice = (card: Card) => card.speech?.voice ?? tts.voices[0]?.id;

  let top = 1;
  // Controls inside a card keep their own clicks; anywhere else on it moves the card.
  const CONTROLS = "button, input, textarea, select, label, a, audio, video, .menu, .grip";

  /** Moves a card with the pointer, kept inside the canvas's top-left edge. */
  function move(e: PointerEvent, card: Card) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(CONTROLS)) return;
    e.preventDefault();
    card.z = ++top;
    document.body.classList.add("moving");
    const node = e.currentTarget as HTMLElement;
    node.setPointerCapture(e.pointerId);
    const start = { x: e.clientX - card.x, y: e.clientY - card.y };
    const drag = (m: PointerEvent) => {
      card.x = Math.max(0, Math.round(m.clientX - start.x));
      card.y = Math.max(0, Math.round(m.clientY - start.y));
    };
    node.addEventListener("pointermove", drag);
    node.addEventListener("pointerup", () => {
      node.removeEventListener("pointermove", drag);
      document.body.classList.remove("moving");
    }, { once: true });
  }

  /** An SVG the agent drew, shown as an image so nothing in it can run. */
  const svgUrl = (svg: string) => `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svg)))}`;

  const edited = (card: Card) => card.confirmed !== undefined && card.text !== card.confirmed;
  /** The script card's note: edits not yet confirmed, then the voiceover's state. */
  function scriptState(card: Card) {
    if (edited(card)) return "Edited. Confirm to update the storyboard.";
    if (card.speech?.clips?.length) return voiced(card) ? `Voiceover ready · ${card.speech.clips.length} scene clips` : "Voiceover out of date";
    if (card.speech?.audio) return voiced(card) ? "Voiceover ready" : "Voiceover out of date";
    return card.confirmed === card.text ? "Confirmed" : "Review, then confirm";
  }

  const scriptFor = (storyboard: Card) => {
    const video = board.cards.find((c) => c.id === storyboard.parent);
    return video && scriptOf(video);
  };
  const voicing = (storyboard: Card) => !!scriptFor(storyboard)?.speech?.busy;
  function videoLabel(storyboard: Card) {
    if (voicing(storyboard)) return "Making voiceover…";
    const script = scriptFor(storyboard);
    const styled = !!styleOf(storyboard) && !!script && scenes(script.text).length > 1;
    if (!tts.connected || !script || voiced(script)) return "Confirm and make video";
    return styled ? "Make scene voices and video" : "Make voiceover and video";
  }
  function videoHint(storyboard: Card) {
    const script = scriptFor(storyboard);
    if (!tts.connected) return "ElevenLabs isn't connected: the video is timed to a natural reading pace until a voiceover is made";
    if (script && !voiced(script)) return "Makes the voiceover with the script card's voice first, then builds the video";
    return "Builds the video with the voiceover you made";
  }

  /** Makes the voiceover first when it is missing or older than the script, then builds the video. */
  async function makeVideo(storyboard: Card) {
    const script = scriptFor(storyboard);
    // A styled video gets one clip per scene when the voice is made here; a voiceover
    // already made from this script is reused as it is, whatever the style.
    const styled = !!styleOf(storyboard) && !!script && scenes(script.text).length > 1;
    if (tts.connected && script && !voiced(script)) {
      await (styled ? speakScenes(script) : speak(script));
      if (!voiced(script)) {
        storyboard.status = "The voiceover could not be made; see the script card.";
        return;
      }
    }
    onconfirm(storyboard);
  }

  /** Each preview's follow-up choice: true changes the words too. */
  type FollowMode = "visuals" | "words" | "rebuild";
  let followMode = $state<Record<number, FollowMode>>({});
  const FOLLOW: { mode: FollowMode; label: string; title: string; placeholder: string; note: string }[] = [
    { mode: "visuals", label: "Visuals only", title: "Changes scenes and visuals; the script stays and this session's voiceover is used", placeholder: "Such as: make the scene 4 chart bigger", note: "Keeps the script and uses this session's voiceover." },
    { mode: "words", label: "Change the words", title: "Changes the script; the voiceover is made again and the video rebuilt", placeholder: "Such as: make scene 3 shorter and mention Copilot", note: "Makes a new script and voiceover, then rebuilds the video." },
    { mode: "rebuild", label: "Rebuild", title: "A new storyboard and a new video from the confirmed script, with this session's voiceover", placeholder: "Optional: what to do differently this time", note: "Redoes the storyboard and the video. Keeps the script and this session's voiceover." },
  ];
  const parentOf = (card: Card) => board.cards.find((c) => c.id === card.parent);
  /** The card the agent is working for; its live activity shows on every card that belongs to it. */
  const working = $derived(board.cards.find((c) => c.id === board.active));

  // A clock for the "working" line on busy cards.
  let now = $state(Date.now());
  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const elapsed = (from: number) => {
    const s = Math.max(0, Math.round((now - from) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };

  // After a follow-up that changed the words: once the agent stops, confirm the new
  // script, make its voiceover and rebuild the video.
  $effect(() => {
    const id = board.followUp;
    if (id === undefined || board.active !== undefined) return;
    board.followUp = undefined;
    const storyboard = board.cards.find((c) => c.id === id);
    const script = storyboard && scriptFor(storyboard);
    if (!storyboard || !script) return;
    script.confirmed = script.text;
    void makeVideo(storyboard);
  });

  /** A script the agent wrote for a card: it stays Text to speech and takes no references. */
  const script = (card: Card) => card.kind === "speech" && card.parent !== undefined;

  /** Dragging a card's bottom-right corner sets its size. */
  function resize(e: PointerEvent, card: Card) {
    // Without this the drag selects the card's text.
    e.preventDefault();
    document.body.classList.add("resizing");
    const grip = e.currentTarget as HTMLElement;
    const node = grip.parentElement!;
    grip.setPointerCapture(e.pointerId);
    const start = { x: e.clientX, y: e.clientY, w: node.offsetWidth, h: node.offsetHeight };
    const move = (m: PointerEvent) => {
      card.w = Math.max(260, Math.round(start.w + m.clientX - start.x));
      card.h = Math.max(200, Math.round(start.h + m.clientY - start.y));
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", () => {
      grip.removeEventListener("pointermove", move);
      document.body.classList.remove("resizing");
    }, { once: true });
  }

  /** One ElevenLabs clip per scene of a styled script; the scene headings aren't read. */
  async function speakScenes(card: Card) {
    const model = ttsModel(card), voice = ttsVoice(card);
    const parts = scenes(card.text);
    if (!model || !voice || !parts.length) return;
    card.speech = { ...card.speech, model, voice, busy: true, error: undefined, spoken: card.text, audio: undefined, clips: undefined };
    try {
      const clips: { scene: string; audio: string }[] = [];
      for (const part of parts) {
        const audio = await elevenlabs.speak(part.lines, voice, model);
        clips.push({ scene: part.scene, audio: `data:${audio.mimeType};base64,${audio.data}` });
      }
      card.speech = { ...card.speech, busy: false, clips };
    } catch (e) {
      card.speech = { ...card.speech, busy: false, error: String(e instanceof Error ? e.message : e) };
    }
  }

  async function speak(card: Card) {
    const model = ttsModel(card), voice = ttsVoice(card);
    if (!model || !voice || !card.text.trim()) return;
    card.speech = { ...card.speech, model, voice, busy: true, error: undefined, spoken: card.text };
    try {
      const audio = await elevenlabs.speak(card.text, voice, model);
      card.speech = { ...card.speech, busy: false, audio: `data:${audio.mimeType};base64,${audio.data}` };
    } catch (e) {
      card.speech = { ...card.speech, busy: false, error: String(e instanceof Error ? e.message : e) };
    }
  }

  /** The canvas page: a dotted board of cards, each a video, image or text to speech request. */
  const kinds: { value: Kind; label: string }[] = [
    { value: "video", label: "Video" },
    { value: "image", label: "Image" },
    { value: "speech", label: "Text to speech" },
  ];
  const icons = { video: "video", image: "image", speech: "mic", frames: "image", storyboard: "layout", preview: "play", question: "message-square" } as const;
  const hints = { video: "Describe the video", image: "Describe the image", speech: "Type the text to read aloud", frames: "", storyboard: "", preview: "", question: "" };

  // The card whose model is being chosen.
  let picking = $state<Card | null>(null);


  // A joined card's line runs from its parent's right edge to its own left edge, at the header.
  const links = $derived(
    board.cards.flatMap((c) => {
      const p = board.cards.find((p) => p.id === c.parent);
      return p ? [{ id: c.id, x1: p.x + widthOf(p), y1: p.y + 22, x2: c.x, y2: c.y + 22 }] : [];
    }),
  );

  /** A JPEG still from early in a video, or nothing if the web view can't decode it. */
  function still(url: string): Promise<string | undefined> {
    return new Promise((done) => {
      const video = document.createElement("video");
      const timer = setTimeout(() => done(undefined), 15000);
      const finish = (poster?: string) => {
        clearTimeout(timer);
        video.removeAttribute("src");
        video.load();
        done(poster);
      };
      video.muted = true;
      video.playsInline = true;
      video.preload = "auto";
      video.onerror = () => finish();
      video.onloadeddata = () => (video.currentTime = Math.min(0.5, (video.duration || 1) / 2));
      video.onseeked = () => {
        const scale = Math.min(1, 240 / Math.max(video.videoWidth, video.videoHeight, 1));
        const canvas = document.createElement("canvas");
        canvas.width = Math.round(video.videoWidth * scale);
        canvas.height = Math.round(video.videoHeight * scale);
        canvas.getContext("2d")?.drawImage(video, 0, 0, canvas.width, canvas.height);
        finish(canvas.width ? canvas.toDataURL("image/jpeg", 0.8) : undefined);
      };
      video.src = url;
    });
  }

  /** Adds image and video files as references; anything else is ignored. Each shows a spinner until its preview is ready. */
  function addRefs(card: Card, files: Iterable<File>) {
    for (const file of files) {
      const video = file.type.startsWith("video/");
      if (!video && !file.type.startsWith("image/")) continue;
      card.refs.push({ video, name: file.name, url: URL.createObjectURL(file), loading: true });
      const ref = card.refs[card.refs.length - 1];
      // Saved right away, so the agent can read it and it comes back with the session's canvas.
      void file.arrayBuffer().then((bytes) => saveReference(file.name, new Uint8Array(bytes))).then((path) => (ref.path = path)).catch(() => {});
      if (video) void still(ref.url!).then((poster) => Object.assign(ref, { poster, loading: false }));
    }
  }

  function removeRef(card: Card, ref: Reference) {
    if (ref.url) URL.revokeObjectURL(ref.url);
    card.refs = card.refs.filter((r) => r !== ref);
  }

</script>

<div class="board">
{#if engine.installed}
  <button class="primary newcard" onclick={() => addCard(model, {}, reasoning)} title="New card">New <Icon name="plus" size={14} /></button>
{:else if engine.installed === false}
  <div class="newcard engine">
    <button class="primary" onclick={installEngine} disabled={!!engine.step} title="Downloads Node.js, HyperFrames and the Chrome it renders with, once (about 250 MB)">
      {#if engine.step}<span class="spinner small-spin" aria-hidden="true"></span> {engine.step}{engine.percent != null ? ` ${engine.percent}%` : "…"}{:else}<Icon name="download" size={14} /> Download engine{/if}
    </button>
    {#if engine.error}<p class="error small">{engine.error}</p>{:else if !engine.step}<p class="small muted">Needed once for video. About 250 MB.</p>{/if}
  </div>
{/if}
<div class="canvas" aria-label="Canvas">
  <svg class="links" aria-hidden="true">
    {#each links as l (l.id)}
      {@const mid = (l.x1 + l.x2) / 2}
      <path d="M {l.x1} {l.y1} C {mid} {l.y1}, {mid} {l.y2}, {l.x2} {l.y2}" />
      <circle cx={l.x1} cy={l.y1} r="4" />
      <circle cx={l.x2} cy={l.y2} r="4" />
    {/each}
  </svg>
  {#each board.cards as card (card.id)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="node" class:working={busy(card)} style:width="{widthOf(card)}px" style:height={card.h ? `${card.h}px` : undefined} style:left="{card.x}px" style:top="{card.y}px" style:z-index={card.z} onpointerdown={(e) => move(e, card)} ondragover={(e) => e.preventDefault()} ondrop={(e) => { e.preventDefault(); if (!script(card)) addRefs(card, e.dataTransfer?.files ?? []); }}>
      <button class="iconbtn close" onclick={() => removeCard(card)} title="Remove card" aria-label="Remove card"><Icon name="x" size={14} /></button>
      {#if card.kind === "frames"}
        <header class="frameshead"><Icon name="image" size={14} /> <span class="ellipsis">Frames · {card.text}</span></header>
        {#if card.refs.length}
          <div class="frames">
            {#each card.refs as ref, i (ref.url ?? i)}
              <button class="frame" class:off={ref.off} aria-pressed={!ref.off} title={ref.off ? `${ref.name}: not used as a reference. Click to use it.` : `${ref.name}: used as a style reference. Click to leave it out.`} onclick={() => (ref.off = !ref.off)}>
                <img src={ref.url} alt={ref.name} />
                <span class="caption">{ref.time}</span>
              </button>
            {/each}
          </div>
        {:else if !card.status}
          <div class="cutting"><span class="spinner" role="status" aria-label="Cutting frames"></span> <span class="small muted">Cutting frames…</span></div>
        {/if}
      {:else if card.kind === "question"}
        <header class="frameshead"><Icon name="message-square" size={14} /> <span>The agent has a question</span></header>
        <div class="questions">
          {#each card.questions ?? [] as q, i (i)}
            <fieldset>
              <legend>{q.question}</legend>
              <div class="options">
                {#each q.options as option (option)}
                  <button class="option" class:picked={!q.other && q.answer === option} aria-pressed={!q.other && q.answer === option} onclick={() => { q.other = false; q.answer = option; }}>{option}</button>
                {/each}
                <button class="option" class:picked={q.other} aria-pressed={!!q.other} onclick={() => { q.other = true; q.answer = ""; }}>Other…</button>
              </div>
              {#if q.other}
                <!-- svelte-ignore a11y_autofocus -->
                <input type="text" placeholder="Type your answer" aria-label={`Your answer to: ${q.question}`} bind:value={q.answer} autofocus />
              {/if}
            </fieldset>
          {/each}
        </div>
        <div class="foot">
          <button class="link small" onclick={() => onskip(card)} disabled={!canSend || card.status === "Sending…"} title="Let the agent use its best judgement">Skip</button>
          <span class="grow"></span>
          <button class="primary action" onclick={() => onanswer(card)} disabled={!canSend || !(card.questions ?? []).every((q) => q.answer?.trim()) || card.status === "Sending…"}><Icon name="check" size={12} /> Confirm</button>
        </div>
      {:else if card.kind === "storyboard"}
        <header class="frameshead"><Icon name="layout" size={14} /> <span class="ellipsis grow">{card.text}</span>
          <button class="iconbtn" onclick={() => onredostoryboard(card)} disabled={!canSend || busy(card)} title="Redraw every scene from the confirmed script" aria-label="Redo storyboard"><Icon name="rotate-cw" size={13} /></button>
        </header>
        {#if stale(card)}<p class="small note">The script changed since this was drawn. Confirm the script to redraw it.</p>{/if}
        {#if repeated(card).length}<p class="small note">More than one scene uses {repeated(card).join(", ")}. Redo those scenes so each one has its own layout.</p>{/if}
        <ol class="beats">
          {#each card.beats ?? [] as beat, i (i)}
            <li>
              <img src={svgUrl(beat.svg)} alt={`Scene ${i + 1}`} />
              <p><span class="muted small">{i + 1}</span> {#if beat.component}<span class="comp" class:dup={repeated(card).includes(beat.component.trim().toLowerCase())}>{beat.component}</span> {/if}{beat.line}</p>
              <span class="beatacts">
                <button class="iconbtn" onclick={() => onredobeat(card, i)} disabled={!canSend || busy(card)} title="Redraw only this scene" aria-label={`Redo scene ${i + 1}`}><Icon name="rotate-cw" size={12} /></button>
                <button class="iconbtn" onclick={() => (card.beats = card.beats!.filter((b) => b !== beat))} title="Remove this scene" aria-label={`Remove scene ${i + 1}`}><Icon name="x" size={12} /></button>
              </span>
            </li>
          {/each}
        </ol>
        <div class="foot">
          <span class="small muted">{card.beats?.length ?? 0} scenes</span>
          <span class="grow"></span>
          <button class="primary action" onclick={() => makeVideo(card)} disabled={!canSend || !card.beats?.length || busy(card) || voicing(card)} title={videoHint(card)}><Icon name="check" size={12} /> {videoLabel(card)}</button>
        </div>
      {:else if card.kind === "preview"}
        <header class="frameshead"><Icon name="play" size={14} /> <span class="grow">Video preview</span>
          <button class="iconbtn" onclick={() => (card.reload = (card.reload ?? 0) + 1)} disabled={!card.url} title="Reload the preview" aria-label="Reload preview"><Icon name="rotate-cw" size={13} /></button>
        </header>
        {#if stale(card)}<p class="small note">The script changed since this video was built. Confirm the script and storyboard to rebuild it.</p>{/if}
        {#if card.url}
          {#key card.reload}
            <!-- The agent's local HyperFrames preview; the engine allows only localhost pages here. -->
            <iframe class="preview" src={card.url} title="Video preview" sandbox="allow-scripts allow-same-origin allow-forms"></iframe>
          {/key}
        {/if}
        {#if card.url && !busy(card) && parentOf(card)?.voicedBuild !== true}
          <div class="novoice small">
            <span>Built without this session's voiceover.</span>
            <button class="link small" onclick={() => onfollowup(card, "", "voice")} disabled={!canSend}>Add the voiceover</button>
          </div>
        {/if}
        {#if card.url && card.sync && !busy(card)}
          <div class="novoice small" role="alert">
            <span>{card.sync}</span>
            <button class="link small" onclick={() => onfixsync(card)} disabled={!canSend}>Fix the timing</button>
          </div>
        {/if}
        {#if card.url && !busy(card)}
          <!-- Follow-ups only once the video is built and nothing is running. -->
          <form class="followup" onsubmit={(e) => { e.preventDefault(); const input = e.currentTarget.elements.namedItem("change") as HTMLTextAreaElement; const mode = followMode[card.id] ?? "visuals"; if (input.value.trim() || mode === "rebuild") { onfollowup(card, input.value, mode); input.value = ""; } }}>
            <div class="small strong">Follow up</div>
            <div class="options">
              {#each FOLLOW as option (option.mode)}
                <button type="button" class="option" class:picked={(followMode[card.id] ?? "visuals") === option.mode} aria-pressed={(followMode[card.id] ?? "visuals") === option.mode} onclick={() => (followMode[card.id] = option.mode)} title={option.title}>{option.label}</button>
              {/each}
            </div>
            <div class="row">
              <textarea name="change" rows="2" placeholder={FOLLOW.find((o) => o.mode === (followMode[card.id] ?? "visuals"))!.placeholder} aria-label="Follow-up for this video" disabled={!canSend}></textarea>
              <button class="primary send" disabled={!canSend} title="Send the follow-up" aria-label="Send the follow-up"><Icon name="arrow-up" size={14} /></button>
            </div>
            <p class="small muted">{FOLLOW.find((o) => o.mode === (followMode[card.id] ?? "visuals"))!.note}</p>
          </form>
        {/if}
        <div class="foot">
          {#if card.video}
            <span class="small ellipsis" title={card.video}>Rendered: {card.video.split(/[\\/]/).pop()}</span>
            <button class="link small" onclick={() => onopenvideo(card.video!)}>Open</button>
          {:else}
            <span class="small muted">Not rendered yet</span>
          {/if}
          <span class="grow"></span>
          <button class="primary action" onclick={() => onrender(card)} disabled={!canSend || busy(card)}><Icon name="download" size={12} /> {card.video ? "Render again" : "Render"}</button>
        </div>
      {:else}
      {#if script(card)}
        <header class="frameshead"><Icon name="mic" size={14} /> <span class="grow">Text to speech</span>
          <button class="iconbtn" onclick={() => onredoscript(card)} disabled={!canSend || busy(card)} title="Write a new version of the script" aria-label="Redo script"><Icon name="rotate-cw" size={13} /></button>
        </header>
      {:else}
      <header>
        <Menu label="Card type" value={card.kind} options={kinds} onpick={(k) => (card.kind = k)}>
          {#snippet trigger()}<span class="kind"><Icon name={icons[card.kind]} size={14} /> {kinds.find((k) => k.value === card.kind)?.label}</span>{/snippet}
        </Menu>
      </header>
      {/if}
      {#if !script(card)}
      <div class="refs">
        {#each card.refs as ref, i (ref.url ?? ref.name + i)}
          {#if !ref.url}
            <span class="chip" title={ref.name}><Icon name={ref.video ? "video" : "image"} size={12} /> <span class="ellipsis">{ref.name}</span><button class="iconbtn" onclick={() => removeRef(card, ref)} aria-label={`Remove reference ${i + 1}`} title="Remove"><Icon name="x" size={10} /></button></span>
          {:else}
          <div class="thumb" title={ref.name}>
            {#if ref.video}
              {#if ref.poster}
                <img src={ref.poster} alt={`Reference video ${i + 1}`} />
              {:else if !ref.loading}
                <span class="novideo"><Icon name="video" size={18} /></span>
              {/if}
              <span class="badge"><Icon name="video" size={10} /></span>
            {:else}
              <img src={ref.url} alt={`Reference image ${i + 1}`} class:hidden={ref.loading} onload={() => (ref.loading = false)} onerror={() => (ref.loading = false)} />
            {/if}
            {#if ref.loading}<span class="spinner" role="status" aria-label="Loading"></span>{/if}
            <button class="remove" onclick={() => removeRef(card, ref)} aria-label={`Remove reference ${i + 1}`} title="Remove"><Icon name="x" size={12} /></button>
          </div>
          {/if}
        {/each}
        <label class="add" title="Add images or videos as references, or drop them on the card">
          <Icon name="plus" size={14} />
          <span>Reference</span>
          <input type="file" accept="image/*,video/*" multiple hidden onchange={(e) => { addRefs(card, e.currentTarget.files ?? []); e.currentTarget.value = ""; }} />
        </label>
      </div>
      {/if}
      <textarea rows={card.kind === "speech" && card.text ? 8 : 4} placeholder={hints[card.kind]} aria-label={hints[card.kind]} bind:value={card.text}></textarea>
      {#if script(card)}
        <div class="foot">
          <span class="small" class:muted={!edited(card)} class:note={edited(card)}>{scriptState(card)}</span>
          <span class="grow"></span>
          <button class="primary action" onclick={() => onconfirmscript(card)} disabled={!canSend || busy(card) || !card.text.trim() || card.confirmed === card.text} title="Lock the script in and draw its storyboard"><Icon name="check" size={12} /> {card.confirmed === card.text ? "Confirmed" : "Confirm script"}</button>
        </div>
      {/if}
      {#if card.kind === "speech"}
        {#if !tts.connected}
          <p class="small muted">Connect ElevenLabs to make audio. <button class="link" onclick={onconnect}>Open Connectors</button></p>
        {:else}
          <div class="tts">
            <Menu label="ElevenLabs model" value={ttsModel(card)} options={tts.models.map((m) => ({ value: m.id, label: m.name, hint: m.description }))} onpick={(id) => (card.speech = { ...card.speech, model: id })}>
              {#snippet trigger()}<Icon name="cpu" size={14} /> <span class="ellipsis">{tts.models.find((m) => m.id === ttsModel(card))?.name ?? "Model"}</span>{/snippet}
            </Menu>
            <Menu label="Voice" value={ttsVoice(card)} options={tts.voices.map((v) => ({ value: v.id, label: v.name, hint: v.category ?? undefined }))} onpick={(id) => (card.speech = { ...card.speech, voice: id })}>
              {#snippet trigger()}<Icon name="mic" size={14} /> <span class="ellipsis">{tts.voices.find((v) => v.id === ttsVoice(card))?.name ?? "Voice"}</span>{/snippet}
            </Menu>
            <span class="grow"></span>
            <button class="primary generate" onclick={() => speak(card)} disabled={card.speech?.busy || !card.text.trim() || !ttsModel(card) || !ttsVoice(card)}>
              <Icon name="play" size={12} /> {card.speech?.busy ? "Making…" : card.speech?.audio ? "Again" : "Generate"}
            </button>
          </div>
          {#if card.speech?.audio}<audio controls src={card.speech.audio}></audio>{/if}
          {#if card.speech?.clips?.length}
            <details class="clips">
              <summary class="small muted">{card.speech.clips.length} scene clips</summary>
              {#each card.speech.clips as clip, i (i)}
                <div class="clip"><span class="small ellipsis">{clip.scene}</span><audio controls src={clip.audio}></audio></div>
              {/each}
            </details>
          {/if}
          {#if card.speech?.error ?? tts.error}<p class="error small">{card.speech?.error ?? tts.error}</p>{/if}
        {/if}
      {:else}
        {#if (card.kind === "video" || card.kind === "image") && styles.length}
          <div class="foot">
            <Menu label="Style" title="The house style every step of this video follows" value={card.style ?? ""} options={[{ value: "", label: "No style" }, ...styles.map((s) => ({ value: s.name, label: s.name, hint: s.description }))]} onpick={(name) => (card.style = name || undefined)}>
              {#snippet trigger()}<Icon name="layout" size={14} /> <span class="ellipsis">{card.style ?? "No style"}</span>{/snippet}
            </Menu>
          </div>
        {/if}
        <div class="foot">
          <button class="plain picker model" onclick={() => (picking = card)} title={card.model ? `${card.model.provider}/${card.model.id}` : undefined}>
            <Icon name="cpu" size={14} /> <span class="ellipsis">{card.model?.id ?? "Choose a model"}</span>
          </button>
          {#if (card.levels?.length ?? 0) > 1}
            <Menu label="Reasoning" title="Reasoning" value={card.thinking} options={card.levels!.map((l) => ({ value: l, label: l }))} onpick={(l) => (card.thinking = l)}>
              {#snippet trigger()}<Icon name="zap" size={14} /> <span>{card.thinking}</span>{/snippet}
            </Menu>
          {/if}
          <span class="grow"></span>
          {#if busy(card)}
            <button class="action stop" onclick={onstop} title="Stop the agent"><Icon name="square" size={11} /> Stop</button>
          {:else}
            {#if card.kind === "video" && !scriptOf(card)}
              <button class="primary action" onclick={() => onwrite(card)} disabled={!canSend || !card.text.trim()} title="Write the script only, then stop for review"><Icon name="edit" size={12} /> Write script</button>
            {/if}
            <button class="primary send" onclick={() => onsend(card)} disabled={!canSend || !card.text.trim() || card.status === "Sending…"} title={canSend ? "Send to the agent" : "Open a folder first"} aria-label="Send to the agent"><Icon name="arrow-up" size={14} /></button>
          {/if}
        </div>
      {/if}
      {/if}
      {#if busy(card) && working?.started && working.activity}
        <p class="small live" role="status"><span class="spinner small-spin dark" aria-hidden="true"></span> {working.activity}… <span class="muted">{elapsed(working.started)}</span></p>
      {:else if card.status}<p class="small muted status">{card.status}{#if card.stopped} <button class="link small" onclick={() => oncontinue(card)} disabled={!canSend}>Continue</button>{/if}</p>{/if}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="grip" onpointerdown={(e) => resize(e, card)} ondblclick={() => { card.w = undefined; card.h = undefined; }} title="Drag to resize, double-click to reset"></div>
    </div>
  {:else}
    <p class="empty muted">Click New at the top right to add a card.</p>
  {/each}
</div>
</div>

{#if picking}
  <Models current={picking.model} onclose={() => (picking = null)} onpick={(m) => {
    if (picking) {
      picking.model = { provider: m.provider, id: m.id };
      picking.levels = m.thinkingLevels ?? ["off"];
      if (!picking.levels.includes(picking.thinking ?? "")) picking.thinking = picking.levels[0];
    }
    picking = null;
  }} />
{/if}

<style>
  .board { position: relative; flex: 1; min-height: 0; display: flex; }
  .newcard.engine { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; height: auto; padding: 0; box-shadow: none; max-width: 320px; text-align: right; }
  .engine button { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px; font-size: 13px; box-shadow: 0 2px 8px rgb(0 0 0 / 0.15); }
  .engine p { margin: 0; }
  .small-spin { position: static; width: 12px; height: 12px; border-width: 2px; border-color: rgb(255 255 255 / 0.4); border-top-color: #fff; }
  .newcard { position: absolute; top: 14px; right: 18px; z-index: 5; display: inline-flex; align-items: center; gap: 4px; height: 30px; padding: 0 10px 0 12px; font-size: 13px; box-shadow: 0 2px 8px rgb(0 0 0 / 0.15); }
  .canvas {
    position: relative;
    /* Cards stack among themselves only, so dialogs such as Models stay above them. */
    isolation: isolate;
    flex: 1;
    min-height: 0;
    overflow: auto;
    background-color: var(--bg);
    background-image: radial-gradient(circle, color-mix(in srgb, var(--muted) 45%, transparent) 1.2px, transparent 1.6px);
    background-size: 20px 20px;
  }
  .links { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; pointer-events: none; }
  .links path { fill: none; stroke: var(--muted); stroke-width: 1.5; }
  .links circle { fill: var(--bg); stroke: var(--muted); stroke-width: 1.5; }
  .chip { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; height: 26px; padding: 0 2px 0 8px; border: 1px solid var(--line); border-radius: 6px; font-size: 12px; }
  .chip .iconbtn { padding: 3px; }
  .node {
    position: absolute;
    cursor: grab;
    touch-action: none;
    width: 300px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 10px 0 10px 10px;
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.08);
  }
  /* Card dropdowns open downward and show five items, then scroll. */
  /* While the agent works on a card or the video it belongs to: a raspberry edge that shines along, and a soft pulsing glow. */
  .node.working { animation: glow 2.4s ease-in-out infinite; }
  .node.working::before {
    content: "";
    position: absolute;
    inset: -2px;
    border-radius: inherit;
    padding: 2px;
    background: linear-gradient(110deg, #c2185b 0%, #ff8fb5 20%, #f06292 40%, #c2185b 60%, #ff8fb5 80%, #c2185b 100%);
    background-size: 250% 100%;
    animation: shine 2.4s linear infinite;
    -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
    mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
  }
  @keyframes shine { to { background-position: -250% 0; } }
  @keyframes glow {
    0%, 100% { box-shadow: 0 0 0 0 rgb(240 98 146 / 0), 0 4px 16px rgb(0 0 0 / 0.08); }
    50% { box-shadow: 0 0 22px 2px rgb(240 98 146 / 0.45), 0 4px 16px rgb(0 0 0 / 0.08); }
  }
  @media (prefers-reduced-motion: reduce) {
    .node.working, .node.working::before { animation: none; }
  }
  .node :global(.popover) { top: calc(100% + 6px); bottom: auto; max-height: 172px; }
  header { display: flex; align-items: center; gap: 6px; }
  /* The remove button is a tab on the card's top edge, joined to its top-right corner. */
  .close {
    position: absolute;
    bottom: calc(100% - 1px);
    right: -1px;
    width: 30px;
    height: 26px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-bottom: none;
    border-radius: 8px 8px 0 0;
  }
  .close:hover { background: var(--hover); }
  header :global(.menu) { flex: 1; min-width: 0; }
  header :global(.picker) { width: 100%; padding: 6px 8px; border: 1px solid var(--line); border-radius: 6px; }
  header :global(.popover) { right: 0; max-width: none; }
  .kind { flex: 1; display: inline-flex; align-items: center; gap: 6px; text-align: left; }
  .refs { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .thumb img.hidden { visibility: hidden; }
  .novideo { position: absolute; inset: 0; display: grid; place-items: center; border-radius: 8px; border: 1px solid var(--line); background: var(--hover); color: var(--muted); }
  .spinner { position: absolute; inset: 0; margin: auto; width: 20px; height: 20px; border-radius: 50%; border: 2px solid var(--line); border-top-color: var(--accent, var(--muted)); animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .badge { position: absolute; left: 4px; bottom: 4px; display: grid; place-items: center; width: 16px; height: 16px; border-radius: 4px; background: rgb(0 0 0 / 0.55); color: #fff; }
  .add {
    width: 56px;
    height: 56px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    border: 1px dashed var(--line);
    border-radius: 8px;
    color: var(--muted);
    font-size: 10px;
    cursor: pointer;
  }
  .add:hover { background: var(--hover); color: var(--text); }
  .tts { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .tts :global(.menu) { min-width: 0; }
  .tts :global(.picker) { max-width: 150px; }
  .generate { display: inline-flex; align-items: center; gap: 4px; padding: 4px 10px; font-size: 12px; }
  audio { width: 100%; height: 32px; }
  .clips { display: flex; flex-direction: column; gap: 4px; }
  .clips summary { cursor: pointer; }
  .clip { display: flex; flex-direction: column; gap: 2px; margin-top: 4px; }
  .foot { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .questions { display: flex; flex-direction: column; gap: 12px; overflow: auto; min-height: 0; }
  fieldset { border: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  legend { padding: 0; margin-bottom: 6px; font-weight: 500; line-height: 1.4; }
  .options { display: flex; flex-wrap: wrap; gap: 6px; }
  .option { padding: 5px 10px; border-radius: 999px; font-size: 12px; text-align: left; }
  .option.picked { border-color: var(--accent); color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .action { display: inline-flex; align-items: center; gap: 4px; padding: 4px 10px; font-size: 12px; flex: none; }
  .beats { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; overflow: auto; min-height: 0; flex: 1; }
  .beats li { display: grid; grid-template-columns: 128px 1fr auto; gap: 8px; align-items: start; }
  .beats img { width: 128px; aspect-ratio: 16 / 9; object-fit: contain; background: #fff; border-radius: 6px; border: 1px solid var(--line); }
  .beats p { margin: 0; font-size: 12px; line-height: 1.4; }
  .preview { flex: 1; min-height: 160px; width: 100%; border: 1px solid var(--line); border-radius: 8px; background: #000; }
  .send { width: 28px; height: 28px; padding: 0; display: grid; place-items: center; border-radius: 50%; flex: none; }
  .status { margin: 0; }
  .frameshead { display: flex; align-items: center; gap: 6px; font-weight: 500; min-width: 0; padding-right: 24px; }
  .frames { display: grid; grid-template-columns: repeat(auto-fill, minmax(88px, 1fr)); gap: 6px; overflow: auto; min-height: 0; }
  .frame { margin: 0; padding: 0; position: relative; border: none; background: none; border-radius: 6px; }
  .frame.off img { opacity: 0.3; filter: grayscale(1); }
  .frames img { width: 100%; aspect-ratio: 16 / 9; object-fit: cover; border-radius: 6px; border: 1px solid var(--line); display: block; }
  .frames .caption { position: absolute; left: 4px; bottom: 4px; padding: 0 4px; border-radius: 4px; font-size: 10px; background: rgb(0 0 0 / 0.6); color: #fff; }
  .note { margin: 0; color: var(--accent); }
  .beatacts { display: flex; flex-direction: column; }
  .comp { display: inline-block; font-size: 10px; padding: 0 6px; border-radius: 999px; border: 1px solid var(--line); color: var(--muted); margin-right: 2px; }
  .comp.dup { border-color: var(--accent); color: var(--accent); }
  .stop { display: inline-flex; align-items: center; gap: 4px; padding: 4px 10px; font-size: 12px; }
  .novoice { display: flex; align-items: center; gap: 8px; color: var(--accent); }
  .live { margin: 0; display: flex; align-items: center; gap: 6px; }
  .small-spin.dark { border-color: var(--line); border-top-color: var(--accent); }
  .followup { display: flex; flex-direction: column; gap: 6px; border-top: 1px solid var(--line); padding-top: 8px; }
  .followup .row { display: flex; gap: 6px; align-items: flex-end; }
  .followup textarea { flex: 1; font-size: 12px; min-height: 0; }
  .followup p { margin: 0; }
  .strong { font-weight: 500; }
  .cutting { position: relative; display: flex; align-items: center; gap: 8px; height: 60px; padding-left: 36px; }
  .cutting .spinner { right: auto; left: 8px; }
  .model { align-self: flex-start; min-width: 0; flex: 0 1 auto; overflow: hidden; }
  .model .ellipsis { min-width: 0; }
  .foot > :global(.menu) { flex: none; }
  textarea { resize: none; font: inherit; flex: 1; min-height: 60px; }
  .grip { position: absolute; right: 2px; bottom: 2px; width: 14px; height: 14px; cursor: nwse-resize; touch-action: none; border-radius: 0 0 8px 0;
    background: linear-gradient(135deg, transparent 55%, var(--muted) 55%, var(--muted) 62%, transparent 62%, transparent 75%, var(--muted) 75%, var(--muted) 82%, transparent 82%); opacity: 0.6; }
  .grip:hover { opacity: 1; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; margin: 0; pointer-events: none; }
</style>
