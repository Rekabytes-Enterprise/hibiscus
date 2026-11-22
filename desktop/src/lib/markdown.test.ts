// @vitest-environment jsdom
import { expect, it } from "vitest";
import { cachedMarkdown, markdown } from "./markdown";

it("renders common Markdown", () => {
  const html = markdown("# Title\n\n- **bold** and `code`\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |");
  expect(html).toContain("<h1>Title</h1>");
  expect(html).toContain("<strong>bold</strong>");
  expect(html).toContain("<code>code</code>");
  expect(html).toContain('<code class="hljs language-rust"><span class="hljs-keyword">fn</span> <span class="hljs-title function_">main</span>');
  expect(html).toContain('<div class="code-lang">rust</div>');
  expect(html).toContain('aria-label="Copy code"');
  expect(html).toContain('<div class="table-scroll" tabindex="0" role="region" aria-label="Scrollable table"><table>');
});

it("wraps sanitized HTML tables without changing their headers and cells", () => {
  const root = document.createElement("div");
  root.innerHTML = markdown('<table onclick="alert(1)"><thead><tr><th>Stars</th></tr></thead><tbody><tr><td>131</td></tr></tbody></table>');
  const scroll = root.querySelector(".table-scroll");
  expect(scroll?.querySelector("th")?.textContent).toBe("Stars");
  expect(scroll?.querySelector("td")?.textContent).toBe("131");
  expect(scroll?.querySelector("table")?.hasAttribute("onclick")).toBe(false);
});

it("strips anything that could run or phone home", () => {
  const html = markdown(
    '<script>alert(1)</script><img src=x onerror="alert(2)"><a href="javascript:alert(3)">x</a><style>*{}</style><form><input></form>',
  );
  expect(html).not.toMatch(/<script|onerror|javascript:|<style|<form|<input/);
});

it("escapes code in unknown or missing languages", () => {
  expect(markdown("```nope\n<b>x</b>\n```")).toContain('<code class="hljs">&lt;b&gt;x&lt;/b&gt;');
  expect(markdown("```\na < b\n```")).toContain("a &lt; b");
});

it("remembers rendered text with the same result", () => {
  const text = "**cached** `once`";
  expect(cachedMarkdown(text)).toBe(markdown(text));
  expect(cachedMarkdown(text)).toBe(markdown(text));
});
