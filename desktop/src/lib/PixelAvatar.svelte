<script lang="ts" module>
  // The pixel characters of the Rooxcode account page (frontend PixelAvatar.tsx). Keep the two in sync.
  // 16×16 sprites. Letters are palette keys; "." is transparent.
  const sprites = {
    slime: { name: "Slime", colors: { m: "#f6adc9", l: "#ffd9e8", d: "#e58fb0" }, rows: [
      "................", "................", "......kkkk......", "....kkmmmmkk....",
      "...kmllmmmmmk...", "..kmlmmmmmmmmk..", ".kmmmmmmmmmmmmk.", ".kmmmemmmmemmmk.",
      ".kmmmemmmmemmmk.", ".kmpmmmmmmmmpmk.", ".kmmmmmkkmmmmmk.", "kmmmmmmmmmmmmmmk",
      "kdmmmmmmmmmmmmdk", "kddmmmmmmmmmmddk", ".kkkkkkkkkkkkkk.", "................"] },
    cat: { name: "Cat", colors: { b: "#f4a261", d: "#d9824a", l: "#ffc48f", m: "#fff1e0", n: "#ff8fb3", H: "#f4a261", F: "#d9824a" }, rows: [
      "...k........k...", "..kbk......kbk..", "..kbbkkkkkkbbk..", ".kbbbbbbbbbbbbk.",
      ".kbllbbbbbbbbbk.", ".kbbbebbbbebbbk.", ".kbbbebbbbebbbk.", ".kbpbbbnnbbbpbk.",
      ".kbbbbbkkbbbbbk.", ".kddbbbbbbbbddk.", "..kkkkkkkkkkkk..", "..kHkbmmmmbkHk..",
      "..kHkbmmmmbkHk..", "...kkbbbbbbkk...", "....KFFKKFFK....", "....KKKKKKKK...."] },
    bunny: { name: "Bunny", colors: { b: "#f6eef8", d: "#d9c8e0", i: "#f6adc9", H: "#f6eef8", F: "#d9c8e0" }, rows: [
      "...kk......kk...", "..kbik....kibk..", "..kbik....kibk..", "..kbik....kibk..",
      "..kbbkkkkkkbbk..", ".kbbbbbbbbbbbbk.", ".kbbbebbbbebbbk.", ".kbbbebbbbebbbk.",
      ".kbpbbbbbbbbpbk.", ".kbbbbbkkbbbbbk.", ".kddbbbbbbbbddk.", "..kkkkkkkkkkkk..",
      "..kHkbbbbbbkHk..", "...kkbbbbbbkk...", "....KFFKKFFK....", "....KKKKKKKK...."] },
    frog: { name: "Frog", colors: { g: "#7bc67b", d: "#4e9a5a", l: "#a6e3a1", m: "#e6f5c9", H: "#7bc67b", F: "#4e9a5a" }, rows: [
      "................", "................", "..kkk......kkk..", ".kwwek....kewwk.",
      ".kweek....keewk.", ".kggggggggggggk.", ".kglggggggggggk.", ".kpggggggggggpk.",
      ".kggkkkkkkkkggk.", ".kdggggggggggdk.", "..kkkkkkkkkkkk..", "..kHkmmmmmmkHk..",
      "..kHkmmmmmmkHk..", "...kkggggggkk...", "...KFFFKKFFFK...", "...KKKKKKKKKK..."] },
    ghost: { name: "Ghost", colors: { h: "#efe4f7", d: "#cdbbe0", l: "#ffffff", H: "#efe4f7" }, rows: [
      "....kkkkkkkk....", "...khhhhhhhhk...", "..khhhhhhhhhhk..", ".khlhhhhhhhhhhk.",
      ".khhhhhhhhhhhhk.", ".khhhehhhhehhhk.", ".khhhehhhhehhhk.", ".khphhhhhhhhphk.",
      ".khhhhhkkhhhhhk.", "kHkhhhhhhhhhhkHk", "kHkhhhhhhhhhhkHk", ".kkhhhhhhhhhhkk.",
      "..khhhhhhhhhhk..", "..kdhhhhhhhhdk..", "..khhkkhhkkhhk..", "..kkk..kk..kkk.."] },
    bloom: { name: "Bloom", colors: { r: "#f6adc9", d: "#e57fa6", c: "#fff1e0", g: "#7bc67b", H: "#7bc67b", F: "#4e9a5a" }, rows: [
      ".....kk..kk.....", "...kkrrkkrrkk...", "..krrrrrrrrrrk..", ".krrdkkkkkkdrrk.",
      "krrkcccccccckrrk", "krrkcecccceckrrk", "krrkcecccceckrrk", "krrkpcckkccpkrrk",
      "krrkcccccccckrrk", ".krrdkkkkkkdrrk.", "..krrrrrrrrrrk..", "...kkrrkkrrkk...",
      "..kHk.kggk.kHk..", "...kHkkggkkHk...", "....KFFKKFFK....", "....KKKKKKKK...."] },
  } as const;

  const shared: Record<string, string> = { k: "#2a1a26", K: "#2a1a26", e: "#2a1a26", w: "#ffffff", p: "#ff8fb3" };
</script>

<script lang="ts">
  /** `id` is the avatar the user picked; anything unknown draws the default slime. `still` turns the idle animation off. */
  let { id, size = 22, still = false }: { id: string | null; size?: number; still?: boolean } = $props();

  const sprite = $derived(sprites[(id && id in sprites ? id : "slime") as keyof typeof sprites]);
  // Layers: "K"/"F" are feet that stay planted, "e" eyes blink, the rest is the body, which dips while idle.
  const layers = $derived.by(() => {
    const colors: Record<string, string> = { ...shared, ...sprite.colors };
    const pixels = sprite.rows.flatMap((row, y) =>
      [...row].flatMap((key, x) => (key === "." ? [] : [{ key, x, y, fill: colors[key] }])),
    );
    return {
      feet: pixels.filter((p) => "KF".includes(p.key)),
      body: pixels.filter((p) => !"KFe".includes(p.key)),
      eyes: pixels.filter((p) => p.key === "e"),
    };
  });
</script>

{#snippet rects(pixels: { x: number; y: number; fill: string }[])}
  {#each pixels as p}<rect x={p.x} y={p.y} width="1.02" height="1.02" fill={p.fill} />{/each}
{/snippet}

<svg class="pixel" class:alive={!still} width={size} height={size} viewBox="-1 -1 18 18" shape-rendering="crispEdges" aria-hidden="true">
  {@render rects(layers.feet)}
  <g class="body">
    {@render rects(layers.body)}
    <g class="eyes">{@render rects(layers.eyes)}</g>
  </g>
</svg>

<style>
  /* Same idle as the account site: the body dips one pixel into its planted feet, and the eyes blink. */
  .pixel { display: block; }
  .body, .eyes { transform-box: fill-box; transform-origin: center; }
  .alive .body { animation: dip 1.2s steps(1, end) infinite; }
  .alive .eyes { animation: blink 4.2s steps(1, end) infinite; }
  @keyframes dip { 0%, 49% { transform: translateY(0); } 50%, 100% { transform: translateY(1px); } }
  @keyframes blink { 0%, 92%, 100% { transform: scaleY(1); } 94%, 97% { transform: scaleY(0.15); } }
  @media (prefers-reduced-motion: reduce) {
    .alive .body, .alive .eyes { animation: none; }
  }
</style>
