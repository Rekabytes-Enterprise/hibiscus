// Always-on prose guidance, independent of tool approvals and loop controls.
const section = 'hibiscus_writing_style';
const guidance = `Write clear, direct prose using familiar words. Preserve the meaning,
technical accuracy, and level of detail the user needs.

- Lead with the answer or outcome.
- Describe concrete behavior, mechanisms, and observed results. Avoid vague
  claims about quality, performance, or user experience.
- Use plain words: "use" instead of "utilize" or "leverage", "help" instead of "facilitate".
- Remove filler, unnecessary repetition, excessive hedging, flattery, and
  generic introductions or conclusions.
- State the point directly. Avoid rhetorical framing such as "not just X, but Y"
  and forced groups of three.
- Keep terminology consistent instead of cycling through synonyms.
- Prefer active voice. Split sentences that are difficult to follow.
- Write complete sentences. Do not sacrifice clarity for brevity.
- Use headings and lists when they help readers navigate the answer.
  Use sentence-case headings and bold text sparingly.
- Avoid decorative emojis and em dashes in prose.
- Replace abstract metaphors and jargon with literal descriptions unless the
  technical term is necessary and precise.
- Name sources for attributed claims. Preserve uncertainty when evidence is
  incomplete, and distinguish observations from guesses.
- Report only actions and checks actually performed.

Apply these rules to your own explanations, documentation, and UI copy.
Preserve exact quotations, code syntax, identifiers, commands, and existing
project conventions. Follow the user's requested tone and format when they
differ from these defaults.`;

function append(base) {
  if (base.includes(`<${section}>`)) return base;
  return `${base}${base ? '\n\n' : ''}<${section}>\n${guidance}\n</${section}>`;
}

export default function writingStyle(pi) {
  pi.on('before_agent_start', (event) => {
    const options = event?.systemPromptOptions;
    if (options?.sections && typeof options.sections === 'object' && !Array.isArray(options.sections)) {
      options.sections[section] = guidance;
    } else if (typeof event?.systemPrompt === 'string') {
      return { systemPrompt: append(event.systemPrompt) };
    } else if (options && typeof options === 'object') {
      options.appendSystemPrompt = append(typeof options.appendSystemPrompt === 'string'
        ? options.appendSystemPrompt : '');
    }
  });
}
