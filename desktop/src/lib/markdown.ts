// Assistant replies are Markdown. Model output is untrusted, so the HTML is
// sanitized; the CSP also blocks scripts and remote images.
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import makefile from "highlight.js/lib/languages/makefile";
import md from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import shell from "highlight.js/lib/languages/shell";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import { Marked } from "marked";

// Only languages coding chats commonly use, to keep the app small. Each
// module also registers its aliases (ts, js, sh, py, rs, toml, html, ...).
const languages = { bash, c, cpp, css, diff, dockerfile, go, ini, java, javascript, json, kotlin, makefile, markdown: md, php, python, ruby, rust, shell, sql, swift, typescript, xml, yaml };
for (const [name, language] of Object.entries(languages)) hljs.registerLanguage(name, language);

const escape = (text: string) =>
  text.replace(/[&<>"]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[ch]!);

const parser = new Marked({
  gfm: true,
  renderer: {
    code({ text, lang }) {
      const name = (lang ?? "").trim().split(/\s+/)[0].toLowerCase();
      const known = name && hljs.getLanguage(name) ? name : null;
      const body = known ? hljs.highlight(text, { language: known, ignoreIllegals: true }).value : escape(text);
      const label = name ? `<div class="code-lang">${escape(name)}</div>` : "";
      return `<div class="codeblock">${label}<pre><code class="hljs${known ? ` language-${known}` : ""}">${body}</code></pre></div>`;
    },
  },
});

// Feather "copy" and "check"; the chat view handles the click (.copycode).
const icon = (paths: string) =>
  `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const COPY = `<button class="iconbtn copycode" title="Copy" aria-label="Copy code"><span class="idle">${icon('<rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>')}</span><span class="done">${icon('<polyline points="20 6 9 17 4 12"/>')}</span></button>`;

export function markdown(text: string): string {
  const html = parser.parse(text, { async: false });
  const clean = DOMPurify.sanitize(html, {
    RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select"],
  });
  for (const table of clean.querySelectorAll("table")) {
    const scroll = clean.ownerDocument.createElement("div");
    scroll.className = "table-scroll";
    scroll.tabIndex = 0;
    scroll.setAttribute("role", "region");
    scroll.setAttribute("aria-label", "Scrollable table");
    table.replaceWith(scroll);
    scroll.append(table);
  }
  const container = clean.ownerDocument.createElement("div");
  container.append(clean);
  // Added after sanitizing, which strips buttons; the markup is our own constant.
  return container.innerHTML.replaceAll('<div class="codeblock">', `<div class="codeblock">${COPY}`);
}

// Finished messages never change, so their HTML is kept rather than parsed on
// every render (#159). Streaming text changes on each delta and skips this.
const rendered = new Map<string, string>();
const RENDERED = 1000;

/** `markdown(text)`, remembered for text that has been rendered before. */
export function cachedMarkdown(text: string): string {
  let html = rendered.get(text);
  if (html === undefined) {
    html = markdown(text);
    if (rendered.size >= RENDERED) rendered.delete(rendered.keys().next().value!);
    rendered.set(text, html);
  }
  return html;
}

type Reply = { text: string; streaming: boolean };

/** Mount sanitized reply markup once and update it in place as the text streams. */
export function renderReply(node: HTMLElement, reply: Reply) {
  const template = node.ownerDocument.createElement("template");
  const parse = ({ text, streaming }: Reply) => {
    template.innerHTML = streaming ? markdown(text) : cachedMarkdown(text);
    return template.content;
  };
  node.replaceChildren(parse(reply));
  return {
    update(reply: Reply) {
      updateChildren(node, parse(reply));
    },
  };
}

function sameElement(current: Node, next: Node): current is Element {
  return current instanceof Element && next instanceof Element
    && current.localName === next.localName && current.namespaceURI === next.namespaceURI
    && current.attributes.length === next.attributes.length
    && Array.from(current.attributes).every(({ namespaceURI, localName, value }) =>
      next.getAttributeNS(namespaceURI, localName) === value);
}

function updateChildren(parent: Node, next: Node) {
  let current = parent.firstChild;
  for (const child of Array.from(next.childNodes)) {
    if (!current) {
      parent.appendChild(child);
      continue;
    }
    const sibling = current.nextSibling;
    if (!current.isEqualNode(child)) {
      if (sameElement(current, child)) {
        // Preserve the element itself, including focus and scroll position.
        updateChildren(current, child);
      } else if (current.nodeType === Node.TEXT_NODE && child.nodeType === Node.TEXT_NODE) {
        current.nodeValue = child.nodeValue;
      } else {
        parent.replaceChild(child, current);
      }
    }
    current = sibling;
  }
  while (current) {
    const sibling = current.nextSibling;
    parent.removeChild(current);
    current = sibling;
  }
}
