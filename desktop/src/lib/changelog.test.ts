import { expect, it } from "vitest";
import { parse, RELEASES, releases } from "./changelog";

const sample = `# Changelog

Intro.

## [Unreleased]

- wip

## [0.2.0] - 2026-01-02

### Added

- First thing,
  wrapped onto a second line.
- Second thing.

### Fixed

- A bug.

## [0.1.0]

### Changed

- One.
`;

it("starts at the first released version", () => {
  expect(releases(sample).startsWith("## [0.2.0] - 2026-01-02")).toBe(true);
  expect(releases("no versions yet")).toBe("no versions yet");
});

it("splits releases into sections and items", () => {
  expect(parse(sample)).toEqual([
    {
      version: "0.2.0",
      date: "2026-01-02",
      sections: [
        { title: "Added", items: ["First thing, wrapped onto a second line.", "Second thing."] },
        { title: "Fixed", items: ["A bug."] },
      ],
    },
    { version: "0.1.0", date: null, sections: [{ title: "Changed", items: ["One."] }] },
  ]);
});

it("bundles the repository changelog", () => {
  expect(RELEASES[0].version).toMatch(/^\d+\.\d+\.\d+$/);
  expect(RELEASES[0].sections.length).toBeGreaterThan(0);
});
