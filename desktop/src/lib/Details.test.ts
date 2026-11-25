// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import { apply, empty } from "./chat";
import Details from "./Details.svelte";

it("sends only the selected waiting message and shows which message is steering", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const chat = apply({ ...empty(), running: true }, {
    type: "queue_update",
    prompts: [
      { id: "steering", text: "In progress", images: 0, steer: true },
      { id: "first", text: "Same words", images: 0, steer: false },
      { id: "second", text: "Same words", images: 1, steer: false },
    ],
  });
  const onsendnow = vi.fn(async () => {});
  const view = mount(Details, { target: host, props: {
    folder: "/repo", home: "/", model: null, thinking: "off", stats: null, changes: null,
    diff: [], extensions: [], chat, onsendnow,
    onopenfile: vi.fn(), onshowdiff: vi.fn(), onshowtasks: vi.fn(), onabort: vi.fn(),
  } });
  try {
    const buttons = [...host.querySelectorAll("button")].filter((button) => button.textContent?.trim() === "Send now");
    expect(buttons).toHaveLength(2);
    expect(host.textContent).toContain("Sending next");
    buttons[1]?.click();
    await tick();
    expect(onsendnow).toHaveBeenCalledExactlyOnceWith("second");
  } finally {
    await unmount(view);
    host.remove();
  }
});
