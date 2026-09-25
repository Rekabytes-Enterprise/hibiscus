// Pi owns credential storage/removal. Only provider metadata crosses stdout;
// never serialize credentials or raw exceptions (which may contain secrets).
import { pathToFileURL } from 'node:url';
import { createInterface } from 'node:readline';

const send = record => process.stdout.write(`${JSON.stringify(record)}\n`);
const reader = createInterface({ input: process.stdin, crlfDelay: Infinity });
const requests = [];
let waiting;
reader.on('line', line => {
  try {
    const request = JSON.parse(line);
    if (waiting) { const resolve = waiting; waiting = undefined; resolve(request); }
    else requests.push(request);
  } catch { /* invalid input is never authorization */ }
});
reader.on('close', () => { waiting?.(undefined); waiting = undefined; });
const next = () => requests.length ? Promise.resolve(requests.shift()) : new Promise(resolve => { waiting = resolve; });

try {
  const sdk = await import(pathToFileURL(process.argv[1]).href);
  const runtime = await sdk.ModelRuntime.create({ refreshOnCreate: false, signal: AbortSignal.timeout(15_000) });
  if (typeof runtime.listCredentials !== 'function' || typeof runtime.logout !== 'function') {
    send({ type: 'unavailable' });
  } else {
    const credentials = await runtime.listCredentials({ signal: AbortSignal.timeout(15_000) });
    const providers = credentials.map(({ providerId, type }) => ({ id: providerId, type }))
      .filter(({ id, type }) => typeof id === 'string' && ['oauth', 'api_key'].includes(type))
      .sort((a, b) => a.id.localeCompare(b.id));
    send({ type: 'providers', providers });
    if (providers.length) {
      const request = await next();
      // Rust sends this only after explicit confirmation and closing RPC.
      if (request?.type === 'logout' && providers.some(p => p.id === request.provider)) {
        try {
          await runtime.logout(request.provider, { signal: AbortSignal.timeout(15_000) });
          send({ type: 'done', removed: true });
        } catch (error) {
          if (typeof sdk.CredentialSynchronizationError === 'function' && error instanceof sdk.CredentialSynchronizationError && error.operation === 'logout') {
            send({ type: 'done', removed: true, synchronizationWarning: true });
          } else {
            // A timeout/error can happen after a storage write; do not assert
            // that credentials are unchanged without an authoritative result.
            send({ type: 'done', removed: false });
          }
        }
      }
    }
  }
} catch {
  send({ type: 'failed' });
  process.exitCode = 1;
} finally {
  reader.close();
}
