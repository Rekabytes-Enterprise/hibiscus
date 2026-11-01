#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const { version } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const args = process.argv.slice(2);
if (args.length === 1 && ['--help', '-h'].includes(args[0])) {
  console.log(`Install Hibiscus ${version}\n\nUsage: npx @rekabytes/hibiscus[@version]\n\nInstalls the standalone native Hibiscus binary using the bundled installer.\nThen run: hibiscus\n\nOptions: --help, --version\nEnvironment: HIBISCUS_INSTALL_DIR, HIBISCUS_HOME,\n             HIBISCUS_VERSION, HIBISCUS_REPOSITORY, GITHUB_TOKEN\nRequires Linux/WSL or macOS, curl, tar, and a SHA-256 utility.`);
} else if (args.length === 1 && args[0] === '--version') {
  console.log(version);
} else if (args.length) {
  console.error('hibiscus installer: unexpected arguments; use --help. Start chats with hibiscus after installation.');
  process.exitCode = 2;
} else if (!['darwin', 'linux'].includes(process.platform) || !['x64', 'arm64'].includes(process.arch)) {
  console.error('hibiscus installer: supports Linux/WSL and macOS on x64/arm64. On Windows, run inside WSL.');
  process.exitCode = 1;
} else {
  const result = spawnSync('sh', [fileURLToPath(new URL('../install.sh', import.meta.url))], {
    stdio: 'inherit',
    env: { ...process.env, HIBISCUS_VERSION: process.env.HIBISCUS_VERSION || `v${version}` },
  });
  if (result.error) {
    console.error(`hibiscus installer: could not start installer: ${result.error.message}`);
    process.exitCode = 1;
  } else if (result.signal) {
    process.kill(process.pid, result.signal);
  } else {
    process.exitCode = result.status ?? 1;
  }
}
