// Explicitly loaded by Hibiscus, not by normal standalone Pi. This is a
// best-effort guard for common dangerous shell commands, not a sandbox.
const risky = [
  { label: 'recursive deletion', pattern: /\b(?:rm|unlink|rmdir|shred)\b|\bfind\b[^\n]*\s-delete\b/i },
  { label: 'privileged operation', pattern: /\b(?:sudo|doas|su)\b/i },
  { label: 'destructive git operation', pattern: /\bgit\s+(?:clean|reset\s+--hard|branch\s+-D)\b/i },
  { label: 'system or filesystem change', pattern: /\b(?:mkfs(?:\.[\w-]+)?|dd|shutdown|reboot|poweroff|systemctl|chmod|chown)\b/i },
  { label: 'deployment or publishing', pattern: /\b(?:kubectl\s+delete|terraform\s+(?:apply|destroy)|npm\s+publish|cargo\s+publish|docker\s+(?:system\s+prune|rm)|git\s+push)\b/i },
  { label: 'shell indirection', pattern: /\b(?:sh|bash|zsh|python(?:3)?|perl|ruby|node)\s+(?:-c\b|-e\b|--eval\b|\S+\.(?:sh|py|js|mjs)\b)|\b(?:eval|exec)\s|\b(?:curl|wget)\b[^\n]*\|\s*(?:sh|bash)\b/i },
  { label: 'command substitution', pattern: /\$\(|`/ },
  { label: 'output redirection', pattern: /(?:^|\s)(?:>|>>|\d+>)\s*\S/ },
];

export function reasonFor(command) {
  if (typeof command !== 'string' || !command.trim()) return 'unrecognized shell command';
  return risky.find(rule => rule.pattern.test(command))?.label;
}

export default function (pi) {
  const grants = new Set();
  pi.on('session_start', () => grants.clear());
  pi.on('tool_call', async (event, ctx) => {
    if (event.toolName !== 'bash') return;
    const command = event.input?.command;
    const reason = reasonFor(command);
    if (!reason) return;
    if (typeof command !== 'string' || command.length > 2048) {
      return { block: true, reason: 'Cannot safely display command for approval' };
    }
    // Other extensions are disabled by Hibiscus. Fail closed if RPC has no
    // interactive confirmation path (one-shot, piped, or unsupported UI).
    if (!ctx.hasUI) return { block: true, reason: 'Approval required but no interactive UI is available' };
    const key = JSON.stringify([ctx.cwd, command]);
    if (grants.has(key)) return;
    let decision;
    try {
      decision = await ctx.ui.select(
        `Approval needed: ${reason}\nDirectory: ${JSON.stringify(ctx.cwd)}\nCommand: ${JSON.stringify(command)}`,
        ['Deny', 'Allow', 'Always Allow'],
        { timeout: 60000 },
      );
    } catch {
      return { block: true, reason: 'Approval unavailable' };
    }
    if (decision === 'Always Allow') grants.add(key);
    if (decision !== 'Allow' && decision !== 'Always Allow') {
      return { block: true, reason: 'Dangerous command denied' };
    }
  });
}
