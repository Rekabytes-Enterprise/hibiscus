<script lang="ts">
  // Shows the live page of the engine's browser tool, from the bundled
  // agent-browser's local stream. The user's mouse and keys go back on it.
  import { invoke } from "@tauri-apps/api/core";
  import WebSocket from "@tauri-apps/plugin-websocket";
  import { onDestroy, tick } from "svelte";
  import { commentMessage, MAX_IMAGES, modifiers, toPage, type Hit, type Note } from "./browser";
  import Icon from "./Icon.svelte";

  let {
    folder,
    pointer,
    poke,
    wide,
    running,
    onwide,
    onclose,
    onnotes,
  }: {
    /** The open folder, whose dev server the pane can start. */
    folder: string;
    /** The element Hibi is acting on; `seq` changes for every action. */
    pointer: { target: string; seq: number } | null;
    /** Changes after each browser tool call, to connect once a browser exists. */
    poke: number;
    wide: boolean;
    /** Whether Hibi is working; comments handed to it clear when its run ends. */
    running: boolean;
    onwide: () => void;
    onclose: () => void;
    /** Puts the comments in the message box. */
    onnotes: (text: string, images: { mimeType: string; data: string }[]) => void;
  } = $props();

  let socket: WebSocket | null = null;
  let status = $state<"connecting" | "live" | "none">("connecting");
  let hasFrame = $state(false);
  let page = $state({ width: 0, height: 0 });
  let address = $state("");
  let editing = false;
  let error = $state<string | null>(null);
  let cursor = $state<{ x: number; y: number } | null>(null);
  let clicks = $state(0);
  let screen: HTMLDivElement;
  let canvas = $state<HTMLCanvasElement>();
  let area = $state<HTMLDivElement>();
  // Dev servers found on this computer: null before a search, then the ports.
  let servers = $state<number[] | null>(null);
  let searching = $state(false);

  // Problems on the page (#177): console errors, uncaught exceptions and failed requests.
  type Problem = { kind: "Exception" | "Console" | "Request"; text: string };
  let problems = $state<Problem[]>([]);
  let showProblems = $state(false);

  async function checkProblems() {
    const [logs, errors, requests] = await Promise.all(
      [["console"], ["errors"], ["network", "requests", "--status", "400-599"]].map((a) => run(a).catch(() => null)),
    );
    problems = [
      ...(errors?.errors ?? []).map((e: any): Problem => ({ kind: "Exception", text: e.text })),
      ...(logs?.messages ?? []).filter((m: any) => m.type === "error").map((m: any): Problem => ({ kind: "Console", text: m.text })),
      ...(requests?.requests ?? []).map((q: any): Problem => ({ kind: "Request", text: `${q.status} ${q.method} ${q.url}` })),
    ];
    if (!problems.length) showProblems = false;
  }

  async function clearProblems() {
    await Promise.all([["console", "--clear"], ["errors", "--clear"], ["network", "requests", "--clear"]].map((a) => run(a).catch(() => {})));
    problems = [];
    showProblems = false;
  }

  // shortcut: polls every 3 s while a page is shown; listen to stream events if agent-browser adds them.
  $effect(() => {
    if (status !== "live") return;
    void checkProblems();
    const timer = setInterval(checkProblems, 3000);
    return () => clearInterval(timer);
  });

  // The folder's dev server run by the app (#176), and the command to start one.
  type Server = { command: string; running: boolean; exitCode: number | null; url: string | null };
  let server = $state<Server | null>(null);
  let startCommand = $state<string | null>(null);
  let starting = $state(false);
  let showOutput = $state(false);
  let output = $state<string[]>([]);
  let log = $state<HTMLPreElement>();

  const checkServer = async () => {
    server = await invoke<Server | null>("dev_server_status", { folder }).catch(() => null);
  };

  async function startServer() {
    starting = true;
    error = null;
    try {
      server = await invoke<Server | null>("dev_server_start", { folder });
      if (server?.url) return void go(["open", server.url]);
      error = server?.running ? "The dev server is running but hasn't printed its address yet." : "The dev server stopped. Its output is below.";
      showOutput = true;
    } catch (e) {
      error = String(e);
    } finally {
      starting = false;
    }
  }

  async function stopServer() {
    await invoke("dev_server_stop", { folder }).catch(() => {});
    server = null;
    showOutput = false;
  }

  // The output panel follows the server while it's open.
  $effect(() => {
    if (!showOutput) return;
    const read = async () => {
      output = await invoke<string[]>("dev_server_output", { folder }).catch(() => []);
      await checkServer();
      await tick();
      log?.scrollTo({ top: log.scrollHeight });
    };
    void read();
    const timer = setInterval(read, 1000);
    return () => clearInterval(timer);
  });

  const run = (args: string[]) => invoke<any>("browser", { args });

  // Comment mode (#175): the page takes no clicks or keys. Hovering outlines
  // the element under the pointer, and a click pins a note to it.
  // shortcut: pins keep the place the element had when pinned, so they drift
  // if the page scrolls; track boxes per frame if that bothers people.
  let commenting = $state(false);
  let hover = $state<Hit | null>(null);
  let notes = $state<Note[]>([]);
  let draft = $state<Note | null>(null);
  let draftInput = $state<HTMLInputElement>();
  // Set once the notes are in the message box; they clear when the run they start ends.
  let handed = $state(false);
  let ran = false;
  let hitting = false;

  async function hitAt(e: PointerEvent) {
    const at = toPage(e.clientX, e.clientY, screen.getBoundingClientRect(), page);
    const data = await run(["hit", String(Math.max(0, at.x)), String(Math.max(0, at.y))]).catch(() => null);
    return (data?.result ?? null) as Hit | null;
  }

  /** One lookup at a time, so a fast pointer doesn't queue them up. */
  async function hoverAt(e: PointerEvent) {
    if (hitting) return;
    hitting = true;
    try {
      const found = await hitAt(e);
      if (commenting && !draft) hover = found;
    } finally {
      hitting = false;
    }
  }

  async function pin(e: PointerEvent) {
    const found = await hitAt(e);
    if (!found || !commenting) return;
    hover = null;
    draft = { hit: found, text: "", image: crop(found) };
    await tick();
    draftInput?.focus();
  }

  /** Cuts the element, with a margin, out of the frame on screen, as base64 JPEG. */
  function crop(h: Hit) {
    if (!canvas || !page.width) return "";
    const scale = canvas.width / page.width;
    const x = Math.max(0, (h.x - 16) * scale);
    const y = Math.max(0, (h.y - 16) * scale);
    const w = Math.min(canvas.width - x, (h.width + 32) * scale);
    const ht = Math.min(canvas.height - y, (h.height + 32) * scale);
    if (w < 1 || ht < 1) return "";
    const out = document.createElement("canvas");
    out.width = Math.round(w);
    out.height = Math.round(ht);
    out.getContext("2d")?.drawImage(canvas, x, y, w, ht, 0, 0, out.width, out.height);
    return out.toDataURL("image/jpeg", 0.85).split(",")[1] ?? "";
  }

  function addNote(e: SubmitEvent) {
    e.preventDefault();
    if (!draft?.text.trim()) return;
    notes = [...notes, { ...draft, text: draft.text.trim() }];
    draft = null;
    screen?.focus();
  }

  function leaveComments() {
    commenting = false;
    hover = null;
    draft = null;
  }

  function toggleComments() {
    if (commenting) return leaveComments();
    // Comments already handed to Hibi give way to a new set.
    if (handed) clearNotes();
    commenting = true;
    screen?.focus();
  }

  function clearNotes() {
    notes = [];
    handed = false;
    ran = false;
  }

  function hand() {
    const images = notes.filter((n) => n.image).slice(0, MAX_IMAGES).map((n) => ({ mimeType: "image/jpeg", data: n.image }));
    onnotes(commentMessage(notes), images);
    handed = true;
    leaveComments();
  }

  $effect(() => {
    if (!handed) return;
    if (running) ran = true;
    else if (ran) clearNotes();
  });

  /** Places an overlay on an element's box, in percent of the page. */
  const place = (h: Hit) =>
    `left:${(h.x / page.width) * 100}%;top:${(h.y / page.height) * 100}%;width:${(h.width / page.width) * 100}%;height:${(h.height / page.height) * 100}%`;

  async function disconnect() {
    const old = socket;
    socket = null;
    await old?.disconnect().catch(() => {});
  }

  async function connect() {
    if (socket) return;
    await disconnect();
    try {
      const stream = await run(["stream", "status"]);
      // No Chrome yet (`connected` is false): stay on the start screen until a page opens.
      if (!stream?.enabled || !stream.port || stream.connected === false) {
        status = "none";
        return;
      }
      // Ack pacing: the next frame comes only after this one is drawn, so frames never queue up.
      const ws = await WebSocket.connect(`ws://127.0.0.1:${stream.port}/?pacing=ack&maxFps=30`);
      socket = ws;
      status = "live";
      error = null;
      fit(true);
      ws.addListener((m) => {
        if (m.type === "Close") {
          if (socket === ws) { socket = null; status = "none"; }
          return;
        }
        if (m.type !== "Text") return;
        const msg = JSON.parse(m.data);
        if (msg.type === "frame") void draw(msg);
        else if (msg.type === "url" && !editing) {
          address = msg.url;
        }
      });
    } catch (e) {
      error = String(e);
      status = "none";
    }
  }

  /** Decodes a frame off the main thread, draws it, then asks for the next one. */
  async function draw(msg: any) {
    try {
      const bytes = Uint8Array.from(atob(msg.data), (c) => c.charCodeAt(0));
      const image = await createImageBitmap(new Blob([bytes], { type: "image/jpeg" }));
      if (canvas) {
        if (canvas.width !== image.width || canvas.height !== image.height) {
          canvas.width = image.width;
          canvas.height = image.height;
        }
        canvas.getContext("2d")?.drawImage(image, 0, 0);
      }
      image.close();
      page = { width: msg.metadata.deviceWidth, height: msg.metadata.deviceHeight };
      hasFrame = true;
    } finally {
      send({ type: "ack", seq: msg.seq });
    }
  }

  // Sizes the page to the pane, so it fills it at any window size.
  let sized = "";
  let resizing: ReturnType<typeof setTimeout> | undefined;
  function fit(now = false) {
    clearTimeout(resizing);
    resizing = setTimeout(() => {
      if (!socket || !area) return;
      const size = [Math.max(200, Math.floor(area.clientWidth)), Math.max(200, Math.floor(area.clientHeight))].map(String);
      if (size.join("x") === sized) return;
      sized = size.join("x");
      run(["set", "viewport", ...size]).catch(() => (sized = ""));
    }, now ? 0 : 200);
  }
  $effect(() => {
    if (!area) return;
    const observer = new ResizeObserver(() => fit());
    observer.observe(area);
    return () => observer.disconnect();
  });

  $effect(() => {
    void poke;
    void connect();
    void checkServer();
  });

  // Moves the cursor onto the element, then pulses where Hibi clicks.
  $effect(() => {
    const p = pointer;
    if (!p) return;
    run(["get", "box", p.target]).then(
      (box) => {
        if (pointer?.seq !== p.seq || !box) return;
        cursor = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
        clicks++;
      },
      () => {},
    );
  });

  onDestroy(() => void disconnect());

  const send = (message: object) => socket?.send(JSON.stringify(message)).catch(() => {});
  const BUTTONS = ["left", "middle", "right"];
  let moving = false;

  function mouse(e: PointerEvent | WheelEvent, eventType: string) {
    if (!page.width) return;
    const at = toPage(e.clientX, e.clientY, screen.getBoundingClientRect(), page);
    if (eventType === "mouseWheel") {
      const w = e as WheelEvent;
      return send({ type: "input_mouse", eventType, ...at, deltaX: w.deltaX, deltaY: w.deltaY });
    }
    if (eventType === "mouseMoved") {
      // One move per frame is plenty.
      if (moving) return;
      moving = true;
      requestAnimationFrame(() => (moving = false));
      return send({ type: "input_mouse", eventType, ...at });
    }
    send({ type: "input_mouse", eventType, ...at, button: BUTTONS[e.button] ?? "left", clickCount: e.detail || 1, modifiers: modifiers(e) });
  }

  function key(e: KeyboardEvent, eventType: "keyDown" | "keyUp") {
    e.preventDefault();
    const typed = eventType === "keyDown" && e.key.length === 1 && !e.ctrlKey && !e.metaKey;
    send({
      type: "input_keyboard",
      eventType,
      key: e.key,
      code: e.code,
      ...(typed ? { text: e.key } : {}),
      windowsVirtualKeyCode: e.keyCode,
      modifiers: modifiers(e),
    });
  }

  async function go(args: string[]) {
    error = null;
    try {
      await run(args);
      await connect();
    } catch (e) {
      error = String(e);
    }
  }

  /** Looks for running dev servers, and opens the only one straight away. */
  async function detect() {
    searching = true;
    // The app's own dev server, when it runs from source, isn't the user's app.
    const found = (await invoke<number[]>("dev_servers")).filter((p) => String(p) !== location.port);
    searching = false;
    servers = found;
    if (found.length === 1) void go(["open", `http://localhost:${found[0]}`]);
    if (!found.length) startCommand = await invoke<string | null>("dev_server_detect", { folder }).catch(() => null);
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    editing = false;
    const url = address.trim();
    if (url) void go(["open", /^https?:\/\//.test(url) ? url : `http://${url}`]);
  }
</script>

<aside class="browser" class:wide>
  <header>
    <button class="iconbtn" onclick={() => go(["back"])} disabled={status !== "live"} title="Back" aria-label="Back"><Icon name="arrow-left" size={14} /></button>
    <button class="iconbtn" onclick={() => go(["forward"])} disabled={status !== "live"} title="Forward" aria-label="Forward"><Icon name="arrow-right" size={14} /></button>
    <button class="iconbtn" onclick={() => go(["reload"])} disabled={status !== "live"} title="Reload" aria-label="Reload"><Icon name="rotate-cw" size={14} /></button>
    <button class="iconbtn" class:on={commenting} onclick={toggleComments} disabled={status !== "live"} title={commenting ? "Stop commenting (Esc)" : "Comment on an element"} aria-label="Comment on an element" aria-pressed={commenting}><Icon name="message-square" size={14} /></button>
    <form class="address" onsubmit={submit}>
      <input bind:value={address} onfocus={() => (editing = true)} onblur={() => (editing = false)} placeholder="Type a URL" aria-label="Address" spellcheck="false" />
    </form>
    {#if problems.length}
      <button class="problems" class:on={showProblems} onclick={() => (showProblems = !showProblems)} title="Errors on this page" aria-label={`${problems.length} errors on this page`} aria-pressed={showProblems}><Icon name="alert-circle" size={13} />{problems.length}</button>
    {/if}
    {#if server}
      <button class="iconbtn" class:on={showOutput} onclick={() => (showOutput = !showOutput)} title="Dev server output" aria-label="Dev server output" aria-pressed={showOutput}><Icon name="terminal" size={14} /></button>
    {/if}
    <button class="iconbtn" onclick={onwide} title={wide ? "Shrink" : "Expand"} aria-label={wide ? "Shrink" : "Expand"}><Icon name={wide ? "minimize" : "maximize"} size={14} /></button>
    <button class="iconbtn" onclick={onclose} title="Close" aria-label="Close browser"><Icon name="x" size={14} /></button>
  </header>
  {#if error}<div class="note error">{error}</div>{/if}
  {#if status === "live"}
    <div class="viewport" bind:this={area}>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="screen"
        class:commenting
        bind:this={screen}
        tabindex="0"
        role="application"
        aria-label="Browser page"
        style:--ratio={page.width ? page.width / page.height : 16 / 9}
        onpointerdown={(e) => {
          screen.focus();
          if (commenting) return void (draft ? (draft = null) : pin(e));
          screen.setPointerCapture(e.pointerId);
          mouse(e, "mousePressed");
        }}
        onpointerup={(e) => !commenting && mouse(e, "mouseReleased")}
        onpointermove={(e) => (commenting ? !draft && hoverAt(e) : mouse(e, "mouseMoved"))}
        onpointerleave={() => (hover = null)}
        onwheel={(e) => mouse(e, "mouseWheel")}
        oncontextmenu={(e) => e.preventDefault()}
        onkeydown={(e) => (commenting ? e.key === "Escape" && leaveComments() : key(e, "keyDown"))}
        onkeyup={(e) => !commenting && key(e, "keyUp")}
      >
        <canvas bind:this={canvas}></canvas>
        {#if !hasFrame}<span class="waiting muted">Waiting for the page…</span>{/if}
        {#if cursor}
          <div class="cursor" style:left={`${(cursor.x / page.width) * 100}%`} style:top={`${(cursor.y / page.height) * 100}%`}>
            {#key clicks}<span class="ripple"></span>{/key}
            <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 2l16 10-7 1.5L9.5 21z" /></svg>
          </div>
        {/if}
        {#if page.width}
          {#if hover}<div class="outline" style={place(hover)}></div>{/if}
          {#each notes as note, i}<div class="outline pinned" style={place(note.hit)} title={note.text}><span class="pin">{i + 1}</span></div>{/each}
          {#if draft}
            <div class="outline" style={place(draft.hit)}></div>
            <!-- Keys and clicks in the note stay out of the page. -->
            <form
              class="draft"
              style:left="min({(draft.hit.x / page.width) * 100}%, calc(100% - 276px))"
              style:top="min({((draft.hit.y + draft.hit.height) / page.height) * 100}%, calc(100% - 48px))"
              onsubmit={addNote}
              onpointerdown={(e) => e.stopPropagation()}
              onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") { draft = null; screen.focus(); } }}
              onkeyup={(e) => e.stopPropagation()}
            >
              <input bind:this={draftInput} bind:value={draft.text} placeholder="What should change?" aria-label="Comment" />
              <button class="primary" disabled={!draft.text.trim()}>Add</button>
            </form>
          {/if}
        {/if}
      </div>
    </div>
    {#if commenting || notes.length}
      <div class="comments">
        <span class="muted">
          {#if handed}{notes.length} comment{notes.length > 1 ? "s" : ""} in the message box
          {:else if notes.length}{notes.length} comment{notes.length > 1 ? "s" : ""}
          {:else}Click an element to comment on it{/if}
        </span>
        <span class="grow"></span>
        {#if notes.length}
          <button class="plain" onclick={clearNotes}>Clear</button>
          {#if !handed}<button class="primary" onclick={hand}>Add to message</button>{/if}
        {/if}
      </div>
    {/if}
  {:else}
    <div class="empty">
      <Icon name="globe" size={22} />
      {#if status === "none"}
        <strong>Browse with Hibi</strong>
        <span class="muted">Type a URL, or ask Hibi to open a page. Hibi can read it, click and type.</span>
        <span class="detect">
          {#if searching}
            <span class="muted">Looking for dev servers…</span>
          {:else if servers && servers.length > 1}
            <span class="muted">Open a dev server:</span>
            {#each servers as port (port)}<button class="link" onclick={() => go(["open", `http://localhost:${port}`])}>localhost:{port}</button>{/each}
          {:else if starting}
            <span class="muted">Starting <code>{startCommand}</code>…</span>
          {:else if servers && !servers.length && startCommand}
            <span class="muted">No dev server is running.</span>
            <button class="link" onclick={startServer}>Start <code>{startCommand}</code></button>
          {:else if servers && !servers.length}
            <span class="muted">No dev server found, and this folder has no start command Hibiscus knows. Ask Hibi to start one.</span>
            <button class="link" onclick={detect}>Try again</button>
          {:else}
            <span class="muted">Preview your app instead?</span>
            <button class="link" onclick={detect}>Detect dev server</button>
          {/if}
        </span>
      {:else}
        <span class="muted">Connecting to the browser…</span>
      {/if}
    </div>
  {/if}
  {#if showProblems && problems.length}
    <section class="output">
      <header>
        <strong>Errors on this page</strong>
        <span class="grow"></span>
        <button class="plain" onclick={clearProblems}>Clear</button>
        <button class="iconbtn" onclick={() => (showProblems = false)} title="Hide errors" aria-label="Hide errors"><Icon name="x" size={13} /></button>
      </header>
      <ul class="problemlist">
        {#each problems as p, i (i)}<li><span class="kind">{p.kind}</span><code>{p.text}</code></li>{/each}
      </ul>
    </section>
  {/if}
  {#if showOutput && server}
    <section class="output">
      <header>
        <code class="ellipsis">{server.command}</code>
        <span class="muted">{server.running ? (server.url ?? "running") : `stopped${server.exitCode != null ? ` (exit ${server.exitCode})` : ""}`}</span>
        <span class="grow"></span>
        {#if server.running}<button class="plain" onclick={stopServer}>Stop</button>{/if}
        <button class="iconbtn" onclick={() => (showOutput = false)} title="Hide output" aria-label="Hide output"><Icon name="x" size={13} /></button>
      </header>
      <pre bind:this={log}>{output.join("\n") || "No output yet."}</pre>
    </section>
  {/if}
</aside>
