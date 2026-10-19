// Embedded by Hibiscus. Pi's ModelRuntime performs OAuth, refresh and storage;
// this helper only relays UI requests. Never print credentials to stdout/stderr.
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";

const send = (record) => process.stdout.write(`${JSON.stringify(record)}\n`);
const sdkPath = process.argv[1];
const controller = new AbortController();
const reader = createInterface({ input: process.stdin, crlfDelay: Infinity });
let waiting;
let nextId = 0;
reader.on("line", (line) => {
  let record;
  try { record = JSON.parse(line); } catch { return; }
  if (record.type === "cancel") {
    controller.abort();
    waiting?.reject(new Error("cancelled"));
    waiting = undefined;
  } else if (record.type === "answer" && record.id === waiting?.id) {
    const current = waiting;
    waiting = undefined;
    if (typeof record.value === "string") current.resolve(record.value);
    else current.reject(new Error("cancelled"));
  }
});
reader.on("close", () => controller.abort());

function prompt(question) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    if (question.signal?.aborted || controller.signal.aborted) {
      reject(new Error("cancelled"));
      return;
    }
    const onAbort = () => {
      if (waiting?.id !== id) return;
      waiting = undefined;
      send({ type: "prompt_cancelled", id });
      reject(new Error("cancelled"));
    };
    const finish = (fn) => (value) => {
      question.signal?.removeEventListener("abort", onAbort);
      fn(value);
    };
    waiting = { id, resolve: finish(resolve), reject: finish(reject) };
    question.signal?.addEventListener("abort", onAbort, { once: true });
    send({
      type: "prompt", id, kind: question.type,
      message: question.message,
      options: question.type === "select" ? question.options?.map(({ id, label }) => ({ id, label })) : undefined,
    });
  });
}

try {
  if (!sdkPath) throw new Error("missing Pi SDK");
  const { ModelRuntime } = await import(pathToFileURL(sdkPath).href);
  const runtime = await ModelRuntime.create({ refreshOnCreate: false });
  let provider = process.argv[2] || '';
  const authType = process.argv[3];
  if (!['oauth', 'api_key'].includes(authType)) throw new Error('invalid auth method');
  if (authType === 'api_key') {
    if (typeof runtime.getProviders !== 'function') {
      send({ type: 'unavailable' });
      reader.close();
      process.exit(0);
    }
    const providers = runtime.getProviders().filter(p => p.auth?.apiKey)
      .sort((a, b) => (a.name || a.id).localeCompare(b.name || b.id));
    if (!providers.length) throw new Error('no API key providers');
    provider = await prompt({ type: 'select', message: 'API Key · provider',
      options: providers.map(p => ({ id: p.id, label: p.name || p.id })) });
    if (!providers.some(p => p.id === provider)) throw new Error('invalid provider');
  } else if (!['openai-codex', 'anthropic'].includes(provider)) {
    throw new Error('invalid OAuth provider');
  }
  if (typeof runtime.getProvider === 'function') {
    const supported = runtime.getProvider(provider)?.auth;
    if (!(authType === 'oauth' ? supported?.oauth : supported?.apiKey)) {
      send({ type: 'unavailable' });
      reader.close();
      process.exit(0);
    }
  }
  await runtime.login(provider, authType, {
    signal: controller.signal,
    prompt,
    notify(event) {
      send({
        type: "notice", kind: event.type,
        message: event.message ?? event.instructions,
        url: event.url,
        verificationUri: event.verificationUri,
        userCode: event.userCode,
      });
    },
  });
  send({ type: "done", success: true });
} catch {
  send({ type: "done", success: false });
  process.exitCode = 1;
} finally {
  reader.close();
}
