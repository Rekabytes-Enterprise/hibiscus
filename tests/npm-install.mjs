import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const temp = mkdtempSync(join(tmpdir(), 'hibiscus-npm-'));
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const version = readFileSync(join(root, 'Cargo.toml'), 'utf8').match(/^version = "([^"]+)"/m)[1];
assert.equal(pkg.version, version);
const env = { ...process.env, npm_config_cache: join(temp, 'cache'), npm_config_registry: 'http://127.0.0.1:1', npm_config_audit: 'false', npm_config_fund: 'false' };
delete env.HIBISCUS_VERSION;
function run(command, args, overrides = {}) {
  return spawnSync(command, args, { cwd: root, env: { ...env, ...overrides }, encoding: 'utf8', timeout: 30000 });
}
function success(result) {
  assert.equal(result.status, 0, result.stderr || String(result.error));
  return result.stdout;
}
try {
  const pack = JSON.parse(success(run('npm', ['pack', '--offline', '--ignore-scripts', '--json', '--pack-destination', temp])))[0];
  assert.deepEqual(pack.files.map(f => f.path).sort(), ['LICENSE', 'README.md', 'install.sh', 'npm/install.mjs', 'package.json'].sort());
  const tarball = join(temp, pack.filename);
  const execArgs = ['exec', '--offline', '--yes', '--ignore-scripts', `--package=${tarball}`, '--', 'hibiscus-install'];
  assert.equal(success(run('npm', [...execArgs, '--version'])).trim(), version);
  assert.match(success(run('npm', [...execArgs, '--help'])), /Then run: hibiscus/);
  assert.equal(run('npm', [...execArgs, 'unexpected']).status, 2);

  const bin = join(temp, 'bin');
  mkdirSync(bin);
  // Run the packed npx entry point, but substitute only the shell installer.
  writeFileSync(join(bin, 'sh'), '#!/bin/sh\nprintf "%s\\n%s\\n" "$HIBISCUS_VERSION" "$1" > "$HIBISCUS_TEST_LOG"\nexit "${HIBISCUS_TEST_EXIT:-0}"\n', { mode: 0o755 });
  // npm itself uses a shell to run the bin: reserve the fake sh for the Node child.
  success(run('tar', ['-xzf', tarball, '-C', temp]));
  const overrides = { PATH: `${bin}:${process.env.PATH}`, HIBISCUS_TEST_LOG: join(temp, 'log') };
  success(run(process.execPath, [join(temp, 'package/npm/install.mjs')], overrides));
  const lines = readFileSync(join(temp, 'log'), 'utf8').trim().split('\n');
  assert.equal(lines[0], `v${version}`);
  assert.equal(realpathSync(lines[1]), realpathSync(join(temp, 'package/install.sh')));
  const failed = run(process.execPath, [join(temp, 'package/npm/install.mjs')], { ...overrides, HIBISCUS_VERSION: 'v0.1.0', HIBISCUS_TEST_EXIT: '7' });
  assert.equal(failed.status, 7);
  assert.equal(readFileSync(join(temp, 'log'), 'utf8').split('\n')[0], 'v0.1.0');
  console.log('npm package, offline npx entry point, version pinning and failure propagation passed');
} finally {
  rmSync(temp, { recursive: true, force: true });
}
