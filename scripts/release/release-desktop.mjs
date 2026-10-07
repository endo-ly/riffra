import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const tauriConfPath = join(repositoryRoot, 'apps/desktop/src-tauri/tauri.conf.json');
const bundleDir = join(repositoryRoot, 'target/release/bundle/nsis');

function runGit(...arguments_) {
  return execFileSync('git', arguments_, { cwd: repositoryRoot, encoding: 'utf8' }).trim();
}

/** CalVer release identifier: the local date as major.minor.patch. */
function releaseVersion(now = new Date()) {
  return `${now.getFullYear()}.${now.getMonth() + 1}.${now.getDate()}`;
}

function ensureTagIsNew(tag) {
  if (runGit('ls-remote', '--tags', 'origin', `refs/tags/${tag}`) !== '') {
    throw new Error(
      `Tag ${tag} already exists on the remote. CalVer allows one release per day; ` +
        'replace the existing release manually or wait for the next day.',
    );
  }
}

/** Official releases are always cut from the default branch. */
function ensureReleaseBranch() {
  const branch = runGit('rev-parse', '--abbrev-ref', 'HEAD');
  if (branch !== 'main') {
    throw new Error(`Desktop releases must be created from main, not from ${branch}.`);
  }
}

/** A release build must come from a committed state so the tag can point at it. */
function ensureCleanWorkingTree() {
  if (runGit('status', '--porcelain') !== '') {
    throw new Error(
      'The working tree has uncommitted changes. Commit or stash them before releasing.',
    );
  }
}

function stampVersion(version) {
  const conf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
  if (conf.version === version) return false;
  conf.version = version;
  writeFileSync(tauriConfPath, `${JSON.stringify(conf, null, 2)}\n`);
  return true;
}

function ensureTagIsAbsentLocally(tag) {
  try {
    execFileSync('git', ['rev-parse', '--verify', '--quiet', `refs/tags/${tag}`], {
      cwd: repositoryRoot,
      stdio: 'ignore',
    });
  } catch {
    return;
  }
  throw new Error(
    `Local tag ${tag} already exists from an earlier attempt. ` +
      `If it points at the commit to release, remove it with "git tag -d ${tag}" and rerun.`,
  );
}

function readGithubRepository() {
  const remote = runGit('remote', 'get-url', 'origin');
  const match = remote.match(/github\.com[/:]([^/]+)\/([^/.]+?)(?:\.git)?$/);
  if (!match) {
    throw new Error('The origin remote is not a GitHub repository.');
  }
  return `${match[1]}/${match[2]}`;
}

function publishBundle(version, tag) {
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
            url: `https://github.com/${repository}/releases/download/${tag}/${installerName}`,
          },
        },
      },
      null,
      2,
    )}\n`,
  );

  runGit('push', 'origin', 'main');
  runGit('push', 'origin', tag);
  execFileSync(
    'gh',
    [
      'release',
      'create',
      tag,
      installerPath,
      latestJsonPath,
      '--title',
      tag,
      '--generate-notes',
      '--verify-tag',
      '--latest',
    ],
    { cwd: repositoryRoot, stdio: 'inherit' },
  );
}

function main() {
  ensureReleaseBranch();
  ensureCleanWorkingTree();
  const version = releaseVersion();
  const tag = `v${version}`;
  ensureTagIsNew(tag);
  const stamped = stampVersion(version);
  if (stamped) {
    runGit('add', 'apps/desktop/src-tauri/tauri.conf.json');
    runGit('commit', '-m', `chore(release): ${tag}`);
  }
  ensureTagIsAbsentLocally(tag);
  runGit('tag', tag);

  execFileSync('node', [join(repositoryRoot, 'scripts/dev/build-desktop.mjs')], {
    cwd: repositoryRoot,
    stdio: 'inherit',
  });
  publishBundle(version, tag);

  console.log(`Published Riffra ${tag}. Installed applications will pick it up.`);
  if (stamped) {
    console.log(
      `The version bump was committed to ${runGit('rev-parse', '--abbrev-ref', 'HEAD')}.`,
    );
  }
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
