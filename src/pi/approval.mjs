import { existsSync } from 'node:fs';

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

// Extract simple shell commands without granting a wildcard for syntax we do
// not understand. Comments outside quotes are not operations; compound calls
// need every risky command authorized independently. This is not a shell AST.
export function shellCommands(command) {
  if (/\$\(|`|<<|\\\n|[{}()]/.test(command) ||
      /(?:^|[;&|\n])\s*(?:if|then|else|elif|fi|for|while|until|do|done|case|esac|function)(?:\s|$)/.test(command)) {
    return [command.trim()];
  }
  const parts = [];
  let start = 0;
  let quote = '';
  for (let i = 0; i < command.length; i++) {
    const ch = command[i];
    if (ch === '\\' && quote !== "'") { i++; continue; }
    if (quote) { if (ch === quote) quote = ''; continue; }
    if (ch === "'" || ch === '"') { quote = ch; continue; }
    if (ch === '#' && (i === 0 || /\s|[;&|]/.test(command[i - 1]))) {
      const part = command.slice(start, i).trim();
      if (part) parts.push(part);
      const newline = command.indexOf('\n', i);
      if (newline === -1) return parts;
      i = newline;
      start = i + 1;
      continue;
    }
    const pair = command.slice(i, i + 2);
    if (ch === '\n' || ch === ';' || ch === '|' || pair === '&&') {
      const part = command.slice(start, i).trim();
      if (part) parts.push(part);
      if (pair === '&&' || pair === '||') i++;
      start = i + 1;
      continue;
    }
    if (ch === '&') return [command.trim()]; // background jobs need exact review
  }
  if (quote) return [command.trim()];
  const last = command.slice(start).trim();
  if (last) parts.push(last);
  return parts;
}

// OpenCode-style command-prefix arity: only the command/subcommand determines
// the suggested grant. A future argument change, including --force or a new
// destination, is included in the grant. Never silently apply this to shell
// prefixes with quoting, assignment, redirection or indirection.
export function permissionFor(command) {
  const words = command.split(/\s+/);
  const arity = { git: 2, kubectl: 2, terraform: 2, npm: 2, cargo: 2, docker: 2,
    'docker system': 3, 'git config': 3, 'git remote': 3, 'git stash': 3 };
  const first = words[0];
  const pair = `${first} ${words[1] ?? ''}`;
  const count = arity[pair] ?? arity[first] ?? 1;
  const prefix = words.slice(0, count);
  const simple = prefix.length === count && prefix.every(word => /^[\w./-]+$/.test(word));
  const exact = !simple || /(?:^|\s)(?:>|>>|\d+>)|[;&|]|\$\(|`|<<|\\/.test(command);
  return exact ? { pattern: command, kind: 'exact' }
    : { pattern: `${prefix.join(' ')} *`, kind: 'prefix', base: prefix.join(' ') };
}

function matches(grant, command) {
  return grant.kind === 'exact' ? grant.pattern === command
    : command === grant.base || command.startsWith(`${grant.base} `);
}

export default function (pi) {
  const loopFlag = new URL('./loop-active', import.meta.url);
  const identity = 'In this client you are presented to the user as Hibiscus. If asked who you are, answer as Hibiscus, the terminal assistant/interface powered by the Pi coding agent. Pi runs the agent, tools, authentication, and sessions; the selected provider/model supplies the intelligence. If asked for the platform, provider, or model, identify them truthfully when known. Do not claim Hibiscus is a model or provider, and do not add a brand disclaimer to unrelated answers.';
  const operational = 'When a request has multiple reasonable interpretations that would materially change the outcome, scope, risk, or cost, do not silently choose one. Use safe read-only inspection to resolve uncertainty where possible; otherwise ask one to three specific questions and wait before making changes. For minor reversible details, use a conservative conventional choice and briefly state your assumption instead of repeatedly asking. Clarify missing scope before destructive, publishing, credential-changing, or unusually costly external actions; tool approval is not a substitute for understanding the request. If the user says not to code yet, inspect and discuss without changing files. Separate observations from guesses and report only checks actually performed. Keep Pi responsible for tools, sessions, authentication, and agent behavior.';
  const guidance = { hibiscus_identity: identity, hibiscus_operational: operational };
  function appendGuidance(base) {
    let result = base;
    for (const [name, text] of Object.entries(guidance)) {
      if (!result.includes(`<${name}>`)) {
        result += `${result ? '\n\n' : ''}<${name}>\n${text}\n</${name}>`;
      }
    }
    return result;
  }
  // Keep Pi's prompt/tools intact. Some Pi versions provide prompt options but
  // no sections object (as reported on macOS). Older hooks may expose only the
  // rendered prompt; append to that text without dropping its base instructions.
  pi.on('before_agent_start', (event) => {
    const options = event?.systemPromptOptions;
    if (options?.sections && typeof options.sections === 'object' && !Array.isArray(options.sections)) {
      Object.assign(options.sections, guidance);
      return;
    }
    if (typeof event?.systemPrompt === 'string') {
      // Older hooks can have prompt options without structured sections. Use
      // their supported full-prompt result, preserving Pi's original text.
      return { systemPrompt: appendGuidance(event.systemPrompt) };
    }
    if (options && typeof options === 'object') {
      // Last-resort options-only shape: preserve any existing appended rules.
      const existing = typeof options.appendSystemPrompt === 'string' ? options.appendSystemPrompt : '';
      options.appendSystemPrompt = appendGuidance(existing);
    }
  });
  const grants = [];
  // Tool calls can run in parallel. Serialize decisions so an "always" answer
  // covers requests already waiting for the same pattern; "once" does not.
  let approvalQueue = Promise.resolve();
  let queuedCount = 0;
  let deniedThrough = 0;
  let steps = [];
  pi.on('session_start', (_event, ctx) => {
    grants.length = 0;
    steps = [];
    // Pi's branch is authoritative when a session is resumed or switched.
    for (const entry of ctx.sessionManager.getBranch()) {
      const message = entry.type === 'message' ? entry.message : undefined;
      const snapshot = message?.role === 'toolResult' && message.toolName === 'goal'
        ? message.details?.hibiscusGoal : undefined;
      if (Array.isArray(snapshot?.steps)) steps = snapshot.steps;
    }
  });
  pi.registerTool({
    name: 'goal', label: 'Goal',
    description: 'Track a task with an explicit numbered checklist. Set the complete list before work; mark a step done only after it is actually finished. Do not infer progress from tool calls. Use clear to remove the checklist.',
    parameters: {
      type: 'object', properties: {
        action: { type: 'string', enum: ['set', 'complete', 'clear'] },
        steps: { type: 'array', items: { type: 'string' }, description: 'Full checklist for set, 1–50 distinct steps' },
        id: { type: 'integer', description: 'One-based step number to complete' },
      }, required: ['action'], additionalProperties: false,
    },
    async execute(_id, params) {
      let error;
      if (params.action === 'set') {
        if (!Array.isArray(params.steps) || !params.steps.length || params.steps.length > 50 ||
          params.steps.some(s => typeof s !== 'string' || !s.trim() || s.length > 200)) {
          error = 'Set requires 1–50 named steps (max 200 characters each).';
        } else {
          steps = params.steps.map(label => ({ label, done: false }));
        }
      } else if (params.action === 'complete') {
        if (!Number.isInteger(params.id) || params.id < 1 || params.id > steps.length) {
          error = 'Step number not in the current checklist.';
        } else if (steps[params.id - 1].done) {
          error = 'Step already complete.';
        } else {
          steps = steps.map((step, index) => index === params.id - 1 ? { ...step, done: true } : step);
        }
      } else if (params.action === 'clear') {
        steps = [];
      } else {
        error = 'Unknown goal action.';
      }
      const completed = steps.filter(step => step.done).length;
      return { content: [{ type: 'text', text: error ?? `Goal: ${completed}/${steps.length} steps completed.` }],
        details: { hibiscusGoal: { steps: steps.map(step => ({ ...step })), completed, total: steps.length, ...(error ? { error } : {}) } } };
    },
  });
  pi.registerTool({
    name: 'loop_status', label: 'Loop status',
    description: 'Only in Hibiscus /loop: report candidate_complete when you judge the requested work and actual tests complete, or blocked with a genuine blocker. Guided mode accepts your assessment without independent verification. Only /loop --strict requires submit_evidence with projectDir/reportPath and a separate review; submit existing current reports before rerunning passing tests. Never print internal status markers in assistant text.',
    parameters: { type: 'object', properties: {
      action: { type: 'string', enum: ['candidate_complete', 'blocked', 'submit_evidence'] },
      reason: { type: 'string', description: 'Required only for blocked; concise explanation' },
      projectDir: { type: 'string', description: 'For submit_evidence: project directory relative to the workspace, or absolute' },
      reportPath: { type: 'string', description: 'For submit_evidence: saved Playwright JSON report path relative to projectDir, or absolute. Submit existing results before rerunning tests.' },
      supportFiles: { type: 'array', maxItems: 8, items: {type:'string'}, description: 'Explicit project-relative test-source, server-log and earlier failure-report files for review. Never include credentials.' },
    }, required: ['action'], additionalProperties: false },
    async execute(_id, params, _signal, _update, ctx) {
      const error = !existsSync(loopFlag) ? 'Loop mode is not active.'
        : params.action === 'blocked' && (typeof params.reason !== 'string' || !params.reason.trim() || params.reason.length > 500)
          ? 'A blocked status needs a concise reason.'
          : params.action === 'submit_evidence' && [params.projectDir, params.reportPath].some(value =>
            typeof value !== 'string' || !value.trim() || value.length > 4096 || /[\x00-\x1f]/.test(value))
            ? 'Evidence submission needs projectDir and reportPath.'
          : params.supportFiles !== undefined && (!Array.isArray(params.supportFiles) || params.supportFiles.length>8 || params.supportFiles.some(p=>typeof p!=='string'||p.length>4096)) ? 'Invalid supportFiles.'
          : !['candidate_complete', 'blocked', 'submit_evidence'].includes(params.action) ? 'Unknown loop action.' : undefined;
      if (!error && params.action === 'submit_evidence') {
        try {
          const response = await ctx.ui.input('Hibiscus internal loop evidence', JSON.stringify(params));
          const result = JSON.parse(response);
          return { content: [{type:'text', text: JSON.stringify(result)}], details: {hibiscusLoop:{state:'submit_evidence',validatedByClient:true}} };
        } catch {
          return { content:[{type:'text',text:'Evidence validation bridge unavailable; no success was recorded.'}], details:{hibiscusLoop:{error:'Evidence bridge unavailable'}} };
        }
      }
      return { content: [{ type: 'text', text: error ?? 'Loop status noted; Hibiscus will review the result.' }],
        details: { hibiscusLoop: error ? { error } : {
          state: params.action, ...(params.action === 'blocked' ? { reason: params.reason.trim() } : {}),
          ...(params.action === 'submit_evidence' ? { projectDir: params.projectDir, reportPath: params.reportPath } : {}),
        } } };
    },
  });
  pi.on('tool_call', async (event, ctx) => {
    if (event.toolName !== 'bash') return;
    if (existsSync(loopFlag)) return; // Explicit /loop scope; normal chats remain gated.
    const command = event.input?.command;
    const operations = typeof command === 'string'
      ? shellCommands(command).map(text => ({ text, reason: reasonFor(text), rule: permissionFor(text) }))
        .filter(item => item.reason)
      : [{ text: '', reason: 'unrecognized shell command' }];
    if (typeof command === 'string' && !command.trim()) {
      return { block: true, reason: 'Cannot safely display command for approval' };
    }
    if (!operations.length) return;
    if (typeof command !== 'string' || command.length > 2048) {
      return { block: true, reason: 'Cannot safely display command for approval' };
    }
    // Other extensions are disabled by Hibiscus. Fail closed if RPC has no
    // interactive confirmation path (one-shot, piped, or unsupported UI).
    if (!ctx.hasUI) return { block: true, reason: 'Approval required but no interactive UI is available' };
    const ordinal = ++queuedCount;
    const decide = async () => {
      if (ordinal <= deniedThrough) return { block: true, reason: 'Dangerous command denied' };
      const missing = operations.filter(({ text, rule }) => !grants.some(grant =>
        grant.cwd === ctx.cwd && (rule.kind !== 'exact' || grant.rule.kind === 'exact')
          && matches(grant.rule, text)));
      if (!missing.length) return;
      const patterns = [...new Set(missing.map(item => item.rule.pattern))];
      let decision;
      try {
        decision = await ctx.ui.select(
          `Approval needed: ${missing[0].reason}\nDirectory: ${JSON.stringify(ctx.cwd)}` +
          `\nCommand: ${JSON.stringify(command)}\nAlways allow patterns in this directory for this chat:` +
          `\n${patterns.map(pattern => `  ${pattern}`).join('\n')}` +
          '\n* matches any arguments, including changed targets and force flags.',
          ['Deny', 'Allow', 'Always Allow'],
        );
      } catch {
        deniedThrough = queuedCount;
        return { block: true, reason: 'Approval unavailable' };
      }
      if (decision !== 'Allow' && decision !== 'Always Allow') {
        deniedThrough = queuedCount;
        return { block: true, reason: 'Dangerous command denied' };
      }
      if (decision === 'Always Allow') {
        for (const item of missing) grants.push({ cwd: ctx.cwd, rule: item.rule });
      }
    };
    const request = approvalQueue.then(decide);
    approvalQueue = request.then(() => {}, () => {});
    return request;
  });
}
