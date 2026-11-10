<script lang="ts">
  // Shows the live page of the engine's browser tool, from the bundled
  // agent-browser's local stream. The user's mouse and keys go back on it.
  import { invoke } from "@tauri-apps/api/core";
  import WebSocket from "@tauri-apps/plugin-websocket";
  import { onDestroy } from "svelte";
  import { modifiers, toPage } from "./browser";
  import Icon from "./Icon.svelte";

  let {
    pointer,
    poke,
    wide,
    onwide,
    onclose,
  }: {
    /** The element Hibi is acting on; `seq` changes for every action. */
    pointer: { target: string; seq: number } | null;
    /** Changes after each browser tool call, to connect once a browser exists. */
    poke: number;
    wide: boolean;
    onwide: () => void;
    onclose: () => void;
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

  const run = (args: string[]) => invoke<any>("browser", { args });

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
    <form class="address" onsubmit={submit}>
      <input bind:value={address} onfocus={() => (editing = true)} onblur={() => (editing = false)} placeholder="Type a URL" aria-label="Address" spellcheck="false" />
    </form>
    <button class="iconbtn" onclick={onwide} title={wide ? "Shrink" : "Expand"} aria-label={wide ? "Shrink" : "Expand"}><Icon name={wide ? "minimize" : "maximize"} size={14} /></button>
    <button class="iconbtn" onclick={onclose} title="Close" aria-label="Close browser"><Icon name="x" size={14} /></button>
  </header>
  {#if error}<div class="note error">{error}</div>{/if}
  {#if status === "live"}
    <div class="viewport" bind:this={area}>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="screen"
        bind:this={screen}
        tabindex="0"
        role="application"
        aria-label="Browser page"
        style:--ratio={page.width ? page.width / page.height : 16 / 9}
        onpointerdown={(e) => { screen.focus(); screen.setPointerCapture(e.pointerId); mouse(e, "mousePressed"); }}
        onpointerup={(e) => mouse(e, "mouseReleased")}
        onpointermove={(e) => mouse(e, "mouseMoved")}
        onwheel={(e) => mouse(e, "mouseWheel")}
        oncontextmenu={(e) => e.preventDefault()}
        onkeydown={(e) => key(e, "keyDown")}
        onkeyup={(e) => key(e, "keyUp")}
      >
        <canvas bind:this={canvas}></canvas>
        {#if !hasFrame}<span class="waiting muted">Waiting for the page…</span>{/if}
        {#if cursor}
          <div class="cursor" style:left={`${(cursor.x / page.width) * 100}%`} style:top={`${(cursor.y / page.height) * 100}%`}>
            {#key clicks}<span class="ripple"></span>{/key}
            <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 2l16 10-7 1.5L9.5 21z" /></svg>
          </div>
        {/if}
      </div>
    </div>
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
          {:else if servers && !servers.length}
            <span class="muted">No dev server found on the usual ports. Ask Hibi to start one.</span>
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
</aside>
