import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const verified = spawnSync('bash', [join(root, 'scripts/cargo-verified.sh'), '--version'], { cwd: root, stdio: 'inherit' });
if (verified.status !== 0) process.exit(verified.status ?? 1);
const [operation, ...args] = process.argv.slice(2);
if (!['dev', 'build'].includes(operation)) throw new Error('Use desktop dev or build.');
// Runner/config belong to Tauri, before any user-supplied `--` Cargo arguments.
const child = spawn(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'), operation, ...(operation==='build' ? ['--features','custom-protocol'] : []), '--runner', join(root, 'scripts/cargo-verified.sh'), '--config', join(root, 'apps/desktop/src-tauri/tauri.conf.json'), ...args], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, SKIA_BINARIES_URL: `file://${join(root, '.skia-cache/skia-binaries-{key}.tar.gz')}` },
});
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill(signal));
child.on('exit', (code, signal) => { if (signal) process.kill(process.pid, signal); else process.exit(code ?? 1); });
