# README artwork

- `hibiscus.svg` — five-petal raspberry flower with a terminal chevron. Transparent background; the dark center keeps the chevron readable on light and dark pages. Use with the lowercase `hibiscus` wordmark. The CLI continues using its existing `✿` glyph.
- `terminal-preview.svg` — explicitly labeled illustration with invented sample content, not a captured agent run. Keep its features and shortcuts aligned with the real UI. Do not replace it with private session text, auth URLs, or credentials.

Both SVGs are self-contained, have accessible titles/descriptions, and contain no scripts, external fonts, or embedded remote images. The preview uses system monospace fonts; glyph appearance can vary by renderer.

The README's Shields.io download badge uses GitHub's total release-asset download count. It includes checksums and repeat downloads, excludes source/Cargo installs, and is not a user count. The total-download endpoint ignored an experimental `filter` query during validation; do not label this badge “binary downloads” or claim it excludes checksum files. Counts update through the badge service and may be cached; no repository workflow or analytics collection is needed.
