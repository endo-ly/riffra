import { copyFileSync, existsSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { binariesRoot, getRustHostTriple } from './ensure-desktop.mjs';

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

/** Builds the release CLI and stages it for the Tauri externalBin entry "binaries/riffra". */
function stageCli() {
  const targetTriple = getRustHostTriple();
  const exeSuffix = process.platform === 'win32' ? '.exe' : '';
  execFileSync('cargo', ['build', '-p', 'riffra-cli', '--release'], {
    cwd: repositoryRoot,
    stdio: 'inherit',
  });
  const builtPath = join(repositoryRoot, `target/release/riffra${exeSuffix}`);
  const stagingPath = join(binariesRoot, `riffra-${targetTriple}${exeSuffix}`);
  copyFileSync(builtPath, stagingPath);
}

function main() {
  const privateKey = readUpdaterPrivateKey();
  execFileSync('node', [join(repositoryRoot, 'scripts/dev/ensure-desktop.mjs')], {
    cwd: repositoryRoot,
    stdio: 'inherit',
  });
  stageCli();
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
