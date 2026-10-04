import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const tauriConfPath = join(repositoryRoot, 'apps/desktop/src-tauri/tauri.conf.json');
const bundleDir = join(repositoryRoot, 'target/release/bundle/nsis');

/** CalVer release identifier: the local date as major.minor.patch. */
function releaseVersion(now = new Date()) {
  return `${now.getFullYear()}.${now.getMonth() + 1}.${now.getDate()}`;
}

function stampVersion(version) {
  const conf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
  if (conf.version === version) return false;
  conf.version = version;
  writeFileSync(tauriConfPath, `${JSON.stringify(conf, null, 2)}\n`);
  return true;
}

function ensureTagIsNew(tag) {
  const output = execFileSync('git', ['ls-remote', '--tags', 'origin', `refs/tags/${tag}`], {
    encoding: 'utf8',
  });
  if (output.trim() !== '') {
    throw new Error(
      `Tag ${tag} already exists on the remote. A release was already published today; ` +
        'bump the patch segment manually for another one.',
    );
  }
}

function readGithubRepository() {
  const remote = execFileSync('git', ['remote', 'get-url', 'origin'], { encoding: 'utf8' });
  const match = remote.trim().match(/github\.com[/:]([^/]+)\/([^/.]+?)(?:\.git)?$/);
  if (!match) {
    throw new Error('The origin remote is not a GitHub repository.');
  }
  return `${match[1]}/${match[2]}`;
}

function publishBundle(version) {
  const installerName = `Riffra_${version}_x64-setup.exe`;
  const installerPath = join(bundleDir, installerName);
  const signaturePath = `${installerPath}.sig`;
  const missingFile = [installerPath, signaturePath].find((path) => !existsSync(path));
  if (missingFile) {
    throw new Error(
      `${relative(repositoryRoot, missingFile)} is missing. Run "npm run build:tauri" first.`,
    );
  }

  const repository = readGithubRepository();
  const latestJsonPath = join(bundleDir, 'latest.json');
  writeFileSync(
    latestJsonPath,
    `${JSON.stringify(
      {
        version,
        notes: `Riffra ${version}`,
        pub_date: new Date().toISOString(),
        platforms: {
          'windows-x86_64': {
            signature: readFileSync(signaturePath, 'utf8').trim(),
            url: `https://github.com/${repository}/releases/download/v${version}/${installerName}`,
          },
        },
      },
      null,
      2,
    )}\n`,
  );

  execFileSync(
    'gh',
    [
      'release',
      'create',
      `v${version}`,
      installerPath,
      latestJsonPath,
      '--title',
      `Riffra v${version}`,
      '--generate-notes',
      '--latest',
    ],
    { cwd: repositoryRoot, stdio: 'inherit' },
  );
}

function main() {
  const version = releaseVersion();
  ensureTagIsNew(`v${version}`);
  const stamped = stampVersion(version);

  execFileSync('node', [join(repositoryRoot, 'scripts/dev/build-desktop.mjs')], {
    cwd: repositoryRoot,
    stdio: 'inherit',
  });
  publishBundle(version);

  console.log(`Published Riffra v${version}. Installed applications will pick it up.`);
  if (stamped) {
    console.log(
      `Version ${version} was written to tauri.conf.json. Commit and push it together with the release.`,
    );
  }
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
