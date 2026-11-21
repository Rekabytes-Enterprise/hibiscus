// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import { empty } from "./chat";
import Composer from "./Composer.svelte";

function render(running: boolean) {
  const host = document.createElement("div");
  document.body.append(host);
  const onsend = vi.fn();
  const onabort = vi.fn();
  const composer = mount(Composer, { target: host, props: { running, chat: empty(), onsend, onabort } });

  function button(name: string) {
    const match = [...host.querySelectorAll("button")].find((item) =>
      (item.getAttribute("aria-label") ?? item.textContent?.trim()) === name,
    );
    if (!match) throw new Error(`Missing button ${name}`);
    return match;
  }

  async function draft(text: string) {
    const input = host.querySelector("textarea");
    if (!input) throw new Error("Missing composer input");
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    return input;
  }

  return { host, button, draft, onsend, onabort, async dispose() { await unmount(composer); host.remove(); } };
}

it("steers the selected draft and returns to queue for the next message", async () => {
  const view = render(true);
  try {
    expect(view.host.querySelector('input[type="checkbox"]')).toBeNull();
    expect(view.button("Queue").disabled).toBe(true);
    const menu = view.button("Send mode");
    expect(menu.querySelector("polyline")?.getAttribute("points")).toBe("6 9 12 15 18 9");
    menu.click();
    await tick();
    expect(view.button("Queue for next turn").getAttribute("aria-checked")).toBe("true");
    view.button("Steer current run").click();
    await tick();

    const input = await view.draft("  Change direction  ");
    expect(view.button("Steer").disabled).toBe(false);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(view.onsend).toHaveBeenNthCalledWith(1, "Change direction", [], true);
    expect(view.button("Queue").disabled).toBe(true);

    await view.draft("Continue after this run");
    view.button("Queue").click();
    expect(view.onsend).toHaveBeenNthCalledWith(2, "Continue after this run", [], false);
    view.button("Stop").click();
    expect(view.onabort).toHaveBeenCalledOnce();
  } finally {
    await view.dispose();
  }
});

it("shows regular Send when there is no active run", async () => {
  const view = render(false);
  try {
    expect(view.host.querySelector('[aria-label="Send mode"]')).toBeNull();
    await view.draft("Hello");
    view.button("Send").click();
    expect(view.onsend).toHaveBeenCalledWith("Hello", [], false);
  } finally {
    await view.dispose();
  }
});
