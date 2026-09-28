// Display-only renderer. Never call a model or emit diagnostics on RPC stdout.
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { readFileSync } from 'node:fs';

const sdk = process.argv[1];
try {
  const modulePath = createRequire(sdk).resolve('grok-mermaid');
  const { render } = await import(pathToFileURL(modulePath).href);
  const sources = JSON.parse(readFileSync(0, 'utf8'));
  const result = [];
  let bytes = 0;
  for (const source of sources) {
    if (typeof source !== 'string' || Buffer.byteLength(source) > 8192) continue;
    const art = render(source);
    if (!art || art.warnings.length || art.width > 160 || art.plain.length > 60) continue;
    const lines = art.plain;
    bytes += Buffer.byteLength(JSON.stringify(lines));
    if (bytes > 24000) break;
    result.push({ source, lines });
  }
  process.stdout.write(JSON.stringify(result) + '\n');
} catch {
  process.stdout.write('[]\n');
}
