# Secure-development learning guide

Start with the free [OpenSSF Developing Secure Software course, LFD121](https://openssf.org/training/).
Work through the examples below alongside the course. A course certificate is
optional; understanding the design and its limits is what the Best Practices
knowledge criteria require. Reading this page or having an agent write code does
not by itself meet those criteria.

## Start with Hibiscus's trust model

An asset is something to protect, such as credentials, source files or a release.
A trust boundary is where data moves between components with different authority.
For Hibiscus, those boundaries include provider responses, extension messages,
configuration, credential files and release downloads.

Hibiscus tools and extensions run with your operating-system permissions. There
is no approval gate or execution sandbox. A restricted extension environment
reduces accidental variable inheritance, but a program can still read files that
your account can read. Use a disposable workspace and synthetic credentials for
practice. Read [SECURITY.md](../../SECURITY.md) before exercising tools.

## Learn the principles through actual code

| Principle | Meaning | Hibiscus example and practice |
| --- | --- | --- |
| Least privilege | Give each component only the authority it needs. | Read the release workflow. Identify which job can upload a release and which can request a signing identity. Explain why a compiler should have neither permission. |
| Check before acting | Validate untrusted data before it reaches an operation. | Read `src/extensions/manifest.rs` and the extension tests. Find how an unsupported manifest or invalid tool argument is rejected before execution. |
| Fail closed | On a failed check, stop the protected operation. | Run the credential-store rejection tests below. Check that no provider request is sent and the original store stays unchanged. |
| Separate identity and integrity | Correct bytes alone do not tell you who produced them. | Compare SHA-256 checks with signed release provenance. Explain why an attacker who replaces both an archive and its checksum can defeat a checksum-only download. |
| Minimize shared state | Keep one request or workspace from corrupting another. | Read the checkpoint conflict tests. Explain why restore refuses a file changed after the recorded edit. |

## Recognize common failures

| Failure | What to look for | Current protection and limit |
| --- | --- | --- |
| Command injection | Untrusted text changes the command a shell executes. | Use argument arrays when shell interpretation is unnecessary. Hibiscus's shell tool intentionally executes commands, so trust in its inputs and workspace still matters. |
| Path traversal and symlinks | A name escapes the intended directory or points at another file. | Review path validation, regular-file checks and checked replacement. Explain why checking a path once does not prevent every concurrent filesystem race. |
| Credential leakage | Keys appear in logs, sessions, tool output or child environments. | Private files, masked input and the extension environment policy reduce exposure. Tool output and account-readable files can still contain secrets. |
| OAuth request mix-up | A response belongs to a different sign-in attempt. | State binds the callback to the attempt. PKCE binds the code exchange to its verifier. Read the mismatched-state and redirect tests; neither value is a replacement for TLS. |
| Unbounded input | A parser or process consumes excessive memory, output or time. | Inspect frame, process-output and deadline limits. Fuzzing exercises malformed inputs but does not prove every input is safe. |
| Prompt injection | Instructions in files or provider output influence later actions. | Treat external text as data, not user authorization. Argument schemas validate structure; they cannot establish that an instruction is wanted. |
| Unsafe memory and FFI | Raw pointers or OS calls violate memory or ownership rules. | Review the PTY and process code carefully. Safe Rust and parser fuzzing do not prove every FFI call is correct. |

## Practice and review

Run these against the local test fixtures, which use synthetic data:

```sh
cargo test --locked --test native_runtime auth::
cargo test --locked runtime::oauth::
cargo test --locked --test native_runtime checkpoints::
python3 tests/workflows.py
```

For each example, explain the asset, the untrusted input, where validation happens,
what a rejected operation preserves, and what the protection does not cover.
Read the test assertions as well as their pass/fail output. Review a future change
using those questions and add a test for its failure case before calling it fixed.

After learning and practice, the maintainer should reassess both knowledge rows
in the [Best Practices assessment](openssf-best-practices.md). Keep them Unmet
until the maintainer can explain and apply the principles. Report suspected
vulnerabilities privately; do not post credentials or private report contents.
