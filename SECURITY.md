# Security policy

## Report a vulnerability privately

Use [GitHub's private vulnerability reporting form](https://github.com/Rekabytes-Enterprise/hibiscus/security/advisories/new) to report a suspected vulnerability in Hibiscus. Sign in to GitHub, open the form, and submit the report to the repository maintainers. Do not open a public issue or pull request containing vulnerability details, exploit instructions, or credentials.

Include:

- The affected Hibiscus version or commit, operating system, and installation method.
- A description of the vulnerability, its impact, and the conditions needed to trigger it.
- Minimal reproduction steps or a proof of concept using synthetic data in a workspace you control.
- Relevant logs or screenshots with API keys, OAuth tokens, private code, session contents, and personal information removed.
- Any workaround or suggested fix you have identified.

If the form is unavailable, open a public issue asking only for a private reporting channel. Keep vulnerability details and sensitive attachments out of that issue. Never send a live credential as proof. If a credential has been exposed, revoke it with its provider.

## Supported versions

Security fixes target the [latest stable GitHub release](https://github.com/Rekabytes-Enterprise/hibiscus/releases/latest). Upgrade to that release before testing whether a problem still occurs, when it is safe to do so. Older releases do not receive a separate security backport stream. Reports affecting older versions or unreleased development code are still welcome; state the exact version or commit.

## Triage and coordinated disclosure

Maintainers use the private report to assess impact, ask for reproduction details, and coordinate a fix and disclosure with the reporter. Response and remediation times depend on maintainer availability and the issue's complexity; there is no guaranteed response deadline or service-level agreement.

Please keep vulnerability details private while that discussion is active. Agree on disclosure timing in the private report, including when a fix or advisory can be published. Discuss delays and any proposed disclosure date there. Acknowledgment or acceptance of a report does not mean a fix has shipped. When a fix is released, the advisory or release notes should identify affected and fixed versions and any available mitigation.

## Execution and data trust model

Hibiscus is a local coding agent. Native tools, shell commands, extension programs, and configured extension checks run with the operating-system permissions of the user who starts it. There is no per-tool approval gate or built-in execution sandbox. Argument validation, process limits, and cancellation do not prevent an invoked program from reading or changing data that the user can access. Run Hibiscus only where that access is appropriate, and inspect extensions before using them. Instructions in repository files, tool results, and provider responses can influence the agent; treat unfamiliar content as untrusted.

Configured providers receive prompts and conversation context sent to them, including file contents returned by tools and attached images. Provider access, retention, and billing follow that provider's policies. Local credentials, sessions, checkpoints, and extension history can contain sensitive information. Keep the Hibiscus data directory private and remove secrets before sharing diagnostics.

Extension processes and their validation checks receive a minimal environment
plus explicitly declared parent variables, described in [extension environment
configuration](docs/extensions.md#environment-variables). This reduces accidental
credential inheritance. It does not prevent programs from reading credential
files, using the network, or printing explicitly supplied secrets. Native shell
tools retain their normal environment.

[Workspace checkpoints](docs/checkpoints.md) cover supported native `write` and `edit` operations on regular workspace files. They do not automatically capture shell or extension side effects, network actions, or every filesystem location. Restore can refuse files changed after the recorded edit. [Extension version history](docs/extensions.md) records managed extension source versions, not every effect of running an extension. Neither feature is a sandbox or a substitute for backups.

Expected execution of a requested command or trusted extension with the user's permissions is part of this design. Reports about validation failures, credential exposure, unintended execution, or other violations of the documented boundaries are welcome. Explain how the observed behavior differs from the documented behavior.
