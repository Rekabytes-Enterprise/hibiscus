<script lang="ts">
  // Shows agent-browser's live stream of the page Hibi is using. Frames come in
  // over the session's local WebSocket; the user's mouse and keys go back on it.
  import { invoke } from "@tauri-apps/api/core";
  import WebSocket from "@tauri-apps/plugin-websocket";
  import { onDestroy } from "svelte";
  import { modifiers, toPage } from "./browser";
  import Icon from "./Icon.svelte";

  let {
    session,
    pointer,
    poke,
    wide,
    onwide,
    onclose,
  }: {
    /** agent-browser session Hibi used last; null is its default one. */
    session: string | null;
    /** The element Hibi is acting on; `seq` changes for every action. */
    pointer: { target: string; seq: number } | null;
    /** Changes after each agent-browser call, to connect once a browser exists. */
    poke: number;
    wide: boolean;
    onwide: () => void;
    onclose: () => void;
  } = $props();

  let socket: WebSocket | null = null;
  let socketSession: string | null = null;
  let status = $state<"connecting" | "live" | "none" | "missing">("connecting");
  let frame = $state("");
  let page = $state({ width: 0, height: 0 });
  let address = $state("");
  let editing = false;
  let error = $state<string | null>(null);
  let cursor = $state<{ x: number; y: number } | null>(null);
  let clicks = $state(0);
  let screen: HTMLDivElement;

  const run = (args: string[]) => invoke<any>("browser", { session, args });

  async function disconnect() {
    const old = socket;
    socket = null;
    await old?.disconnect().catch(() => {});
  }

  async function connect() {
    if (socket && socketSession === session) return;
    await disconnect();
    try {
      const stream = await run(["stream", "status"]);
      if (!stream?.enabled || !stream.port) {
        status = "none";
        return;
      }
      const ws = await WebSocket.connect(`ws://127.0.0.1:${stream.port}/?maxFps=20`);
      socket = ws;
      socketSession = session;
      status = "live";
      error = null;
      ws.addListener((m) => {
        if (m.type === "Close") {
          if (socket === ws) { socket = null; status = "none"; }
          return;
        }
        if (m.type !== "Text") return;
        const msg = JSON.parse(m.data);
        if (msg.type === "frame") {
          frame = `data:image/jpeg;base64,${msg.data}`;
          page = { width: msg.metadata.deviceWidth, height: msg.metadata.deviceHeight };
        } else if (msg.type === "url" && !editing) {
          address = msg.url;
        }
      });
    } catch (e) {
      error = String(e);
      status = error.includes("not installed") ? "missing" : "none";
    }
  }

  $effect(() => {
    void poke;
    void session;
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

  function submit(e: SubmitEvent) {
    e.preventDefault();
    editing = false;
    const url = address.trim();
    if (url) void go(["open", /^https?:\/\//.test(url) ? url : `http://${url}`]);
  }
</script>

<aside class="diff browser" class:wide>
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
  {#if status === "live" && frame}
    <div class="viewport">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="screen"
        bind:this={screen}
        tabindex="0"
        role="application"
        aria-label="Browser page"
        style:--ratio={page.width / page.height}
        onpointerdown={(e) => { screen.focus(); screen.setPointerCapture(e.pointerId); mouse(e, "mousePressed"); }}
        onpointerup={(e) => mouse(e, "mouseReleased")}
        onpointermove={(e) => mouse(e, "mouseMoved")}
        onwheel={(e) => mouse(e, "mouseWheel")}
        oncontextmenu={(e) => e.preventDefault()}
        onkeydown={(e) => key(e, "keyDown")}
        onkeyup={(e) => key(e, "keyUp")}
      >
        <img src={frame} alt="" draggable="false" />
        {#if cursor}
          <div class="cursor" style:left={`${(cursor.x / page.width) * 100}%`} style:top={`${(cursor.y / page.height) * 100}%`}>
            {#key clicks}<span class="ripple"></span>{/key}
            <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 2l16 10-7 1.5L9.5 21z" /></svg>
          </div>
        {/if}
      </div>
    </div>
  {:else if status === "missing"}
    <div class="empty">
      <p>The browser pane shows pages Hibi opens with agent-browser, which isn't installed.</p>
      <p class="muted">Install it in a terminal, then ask Hibi to test something in the browser:</p>
      <pre>npm install -g agent-browser
agent-browser install</pre>
    </div>
  {:else if status === "connecting" || status === "live"}
    <div class="empty muted">{status === "live" ? "Waiting for the page…" : "Connecting to the browser…"}</div>
  {:else}
    <div class="empty">
      <p>No browser is open yet.</p>
      <p class="muted">Ask Hibi to check your app in the browser, or type a URL above.</p>
    </div>
  {/if}
</aside>
