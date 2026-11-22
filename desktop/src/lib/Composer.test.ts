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
  const onpdfsend = vi.fn(async () => {});
  const composer = mount(Composer, { target: host, props: { running, chat: empty(), onsend, onpdfsend, onabort } });

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

  return { host, button, draft, onsend, onpdfsend, onabort, async dispose() { await unmount(composer); host.remove(); } };
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

it("keeps a dropped PDF and draft after setup fails, then sends on retry", async () => {
  const view = render(false);
  try {
    const input = await view.draft("Summarize page 2");
    const file = new File(["%PDF-1.7"], "report.pdf", { type: "application/pdf" });
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [file] } });
    input.dispatchEvent(drop);
    await tick();
    expect(view.button("Remove PDF report.pdf")).toBeDefined();

    view.onpdfsend.mockRejectedValueOnce(new Error("Setup cancelled"));
    view.button("Send").click();
    await vi.waitFor(() => expect(view.host.querySelector('[role="alert"]')?.textContent).toBe("Setup cancelled"));
    expect(input.value).toBe("Summarize page 2");
    expect(view.button("Remove PDF report.pdf")).toBeDefined();
    expect(view.onsend).not.toHaveBeenCalled();

    view.button("Send").click();
    await vi.waitFor(() => expect(input.value).toBe(""));
    expect(view.onpdfsend).toHaveBeenNthCalledWith(2, "Summarize page 2", [], false, [file]);
    expect(view.host.textContent).not.toContain("report.pdf");
  } finally {
    await view.dispose();
  }
});
