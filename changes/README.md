# Release notes for the next version

Every PR adds its release notes here as a new file instead of editing
`CHANGELOG.md`. Separate files never conflict, so PRs merge in any order.

Name the file after the branch, such as `changes/local-ai.md`, and write
the notes the way `CHANGELOG.md` does, under one or more section headings:

```markdown
### Added

- Hibiscus Desktop: what the user can now do, in a sentence or two.

### Fixed

- What no longer goes wrong.
```

Use `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or `Security`.
Other headings are kept after those.

`scripts/bump-version.sh` moves every note into the new version's section
of `CHANGELOG.md`, under matching headings and in file name order, then
deletes the files. This README stays.
