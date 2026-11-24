// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import ChatViewStream from "./ChatViewStream.test-support.svelte";
import { apply, empty, type Chat } from "./chat";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("./engine", () => ({ request: vi.fn() }));

it("keeps the table mounted and focused from row updates through reply settlement", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const props = {
    chat: empty(),
    folder: null,
    answerDisabled: false,
    fallback: null,
    onolder: async () => {},
    onresend: () => {},
    onswitch: () => {},
  };
  const fixture = mount(ChatViewStream, { target: host, props });
  const table = "| Name | Status |\n| --- | --- |\n| First | Ready |";
  const moreRows = "\n| Second | Ready |";
  const prose = "\n\nFollowing prose continues to stream.";
  let chat: Chat = apply(empty(), { type: "agent_start" });
  function delta(text: string) {
    chat = apply(chat, { type: "message_update", assistantMessageEvent: { type: "text_delta", delta: text } });
    flushSync(() => fixture.show(chat));
  }
  const region = () => host.querySelector<HTMLElement>(".table-scroll");
  try {
    const scroller = host.querySelector(".chat");
    if (scroller) scroller.scrollTo = () => {};
    delta(table);
    const scroll = region();
    if (!scroll) throw new Error("Missing scroll region");
    const firstRow = scroll.querySelector("tbody tr");
    scroll.focus();
    scroll.scrollLeft = 120;
    scroll.scrollTop = 64;

    const expectPreserved = () => {
      expect(region()).toBe(scroll);
      expect(scroll.querySelector("tbody tr")).toBe(firstRow);
      expect(scroll.scrollLeft).toBe(120);
      expect(scroll.scrollTop).toBe(64);
      expect(document.activeElement).toBe(scroll);
    };
    delta(moreRows);
    expect(scroll.querySelectorAll("tbody tr")).toHaveLength(2);
    expectPreserved();
    delta(prose);
    delta(" Another sentence.");
    expect(host.textContent).toContain("Another sentence.");
    expectPreserved();

    chat = apply(chat, { type: "message_end", message: {
      role: "assistant", content: [{ type: "text", text: table + moreRows + prose + " Final sentence." }],
    } });
    chat = apply(chat, { type: "agent_settled" });
    flushSync(() => fixture.show(chat));
    await tick();
    expect(host.textContent).toContain("Final sentence.");
    expect(host.textContent).not.toContain("Another sentence.");
    expect(host.querySelector(".assistant")?.classList.contains("streaming")).toBe(false);
    expectPreserved();
  } finally {
    await unmount(fixture);
    host.remove();
  }
});
