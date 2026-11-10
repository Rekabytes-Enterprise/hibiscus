// The release notes shown in Settings → About, from the repository's
// CHANGELOG.md at build time.
import text from "../../../CHANGELOG.md?raw";

export type Section = { title: string; items: string[] };
export type Release = { version: string; date: string | null; sections: Section[] };

/** The changelog from its first released version, without the title and the Unreleased section. */
export function releases(changelog: string): string {
  const at = changelog.search(/^## \[\d/m);
  return at < 0 ? changelog : changelog.slice(at);
}

/** Splits released notes into versions, their sections (Added, Changed…) and bullet items. */
export function parse(changelog: string): Release[] {
  const out: Release[] = [];
  let section: Section | null = null;
  for (const line of releases(changelog).split("\n")) {
    const heading = line.match(/^## \[([^\]]+)\](?:\s*-\s*(\S+))?/);
    if (heading) {
      out.push({ version: heading[1], date: heading[2] ?? null, sections: [] });
      section = null;
    } else if (line.startsWith("### ") && out.length) {
      section = { title: line.slice(4).trim(), items: [] };
      out[out.length - 1].sections.push(section);
    } else if (line.startsWith("- ") && section) {
      section.items.push(line.slice(2).trim());
    } else if (line.trim() && section?.items.length) {
      // A wrapped bullet continues on the next line.
      section.items[section.items.length - 1] += " " + line.trim();
    }
  }
  return out;
}

export const RELEASES = parse(text);
