import { existsSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const desktopRoot = join(repositoryRoot, 'apps/desktop');
const updaterKeyPath = join(repositoryRoot, '.tauri/riffra-updater.key');
const tauriCliPath = join(repositoryRoot, 'node_modules/@tauri-apps/cli/tauri.js');

function ensureUpdaterKey() {
  if (existsSync(updaterKeyPath)) return readFileSync(updaterKeyPath, 'utf8');
  throw new Error(
    'The updater private key is missing. Generate one with:\n' +
      '  npx tauri signer generate -w .tauri/riffra-updater.key --ci',
  );
}

function main() {
  const privateKey = ensureUpdaterKey();
  execFileSync('node', [join(repositoryRoot, 'scripts/dev/ensure-desktop.mjs')], {
    cwd: repositoryRoot,
    stdio: 'inherit',
  });
  execFileSync('node', [tauriCliPath, 'build'], {
    cwd: desktopRoot,
    env: {
      ...process.env,
      TAURI_SIGNING_PRIVATE_KEY: privateKey,
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '',
    },
    stdio: 'inherit',
  });
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
