import { existsSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const desktopRoot = join(repositoryRoot, 'apps/desktop');
const updaterKeyPath = join(repositoryRoot, '.tauri/riffra-updater.key');
const tauriConfPath = join(repositoryRoot, 'apps/desktop/src-tauri/tauri.conf.json');
const tauriCliPath = join(repositoryRoot, 'node_modules/@tauri-apps/cli/tauri.js');

function readUpdaterPrivateKey() {
  if (!existsSync(updaterKeyPath)) {
    throw new Error(
      'The updater private key .tauri/riffra-updater.key is missing. ' +
        'Restore it from a backup; generating a new key pair would break update ' +
        'verification for already installed applications.',
    );
  }
  const conf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
  const keyPubPath = `${updaterKeyPath}.pub`;
  const keyPublic = existsSync(keyPubPath) ? readFileSync(keyPubPath, 'utf8').trim() : '';
  if (!conf.plugins?.updater?.pubkey || conf.plugins.updater.pubkey !== keyPublic) {
    throw new Error(
      'The updater key pair does not match the public key in tauri.conf.json. ' +
        'Restore the key pair the distributed applications were signed with.',
    );
  }
  return readFileSync(updaterKeyPath, 'utf8');
}

function main() {
  const privateKey = readUpdaterPrivateKey();
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
