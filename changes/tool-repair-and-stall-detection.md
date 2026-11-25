### Fixed

- Recover from common tool argument aliases and JSON formatting mistakes after a provider finishes the response. Hibiscus reports repairs and returns unrepairable calls to the model as tool errors.
- Let a single-file edit recover a unique whitespace-only match with consistent indentation. Ambiguous matches leave the file untouched.
- Warn, skip, and stop repeated tool calls that keep failing or returning unchanged file reads. Productive turns can continue across any number of tool rounds.
