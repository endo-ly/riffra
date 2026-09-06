import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const sonalloyRepository = 'https://github.com/endo-ly/sonalloy.git';
const licenseFiles = ['THIRD_PARTY_NOTICES.md', 'LICENSE-MIT', 'LICENSE-APACHE'];
const usage =
  'Usage: node scripts/resources/prepare-builtin.mjs --destination <path> [--source-root <path>] [--release-tag <tag>]';

export function readSonalloyReleaseTag(root = repositoryRoot) {
  const cmakeLists = readFileSync(join(root, 'native/audio-engine/CMakeLists.txt'), 'utf8');
  const configuredVersion = cmakeLists.match(/^\s*set\(RIFFRA_SONALLOY_VERSION\s+"([^"]+)"/m)?.[1];
  const version = configuredVersion;

  if (!version) {
    throw new Error(
      'Sonalloy release version could not be resolved from native/audio-engine/CMakeLists.txt',
    );
  }

  return version.startsWith('v') ? version : `v${version}`;
}

function runGit(args, cwd) {
  execFileSync('git', args, {
    cwd,
    env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
    stdio: 'inherit',
  });
}

function stagePresets(sourcePresets, destination, releaseTag) {
  if (!existsSync(sourcePresets)) {
    throw new Error(`Sonalloy presets directory is missing: ${sourcePresets}`);
  }

  mkdirSync(destination, { recursive: true });
  const presetIds = readdirSync(sourcePresets, { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() && existsSync(join(sourcePresets, entry.name, 'definition.json')),
    )
    .map((entry) => entry.name)
    .sort();

  for (const presetId of presetIds) {
    const presetDestination = join(destination, presetId);
    mkdirSync(presetDestination, { recursive: true });
    copyFileSync(
      join(sourcePresets, presetId, 'definition.json'),
      join(presetDestination, 'definition.json'),
    );
  }

  const sourceAssets = join(sourcePresets, 'assets');
  if (existsSync(sourceAssets)) {
    cpSync(sourceAssets, join(destination, 'assets'), { recursive: true });
  }

  writeFileSync(
    join(destination, 'manifest.json'),
    `${JSON.stringify({ sourceRelease: releaseTag, presets: presetIds }, null, 2)}\n`,
  );
}

function replaceDirectory(destination, stagedDestination) {
  const backupDestination = join(
    dirname(destination),
    `.${basename(destination)}.backup-${randomUUID()}`,
  );
  let movedExistingDestination = false;

  try {
    if (existsSync(destination)) {
      renameSync(destination, backupDestination);
      movedExistingDestination = true;
    }

    renameSync(stagedDestination, destination);

    if (movedExistingDestination) {
      rmSync(backupDestination, { force: true, recursive: true });
    }
  } catch (error) {
    if (movedExistingDestination && !existsSync(destination) && existsSync(backupDestination)) {
      renameSync(backupDestination, destination);
    }
    throw error;
  }
}

export function prepareBuiltinResources({
  destination,
  releaseTag = readSonalloyReleaseTag(),
  sourceRoot,
}) {
  if (!destination) {
    throw new Error('A destination for built-in resources is required.');
  }

  const normalizedReleaseTag = releaseTag.startsWith('v') ? releaseTag : `v${releaseTag}`;
  const destinationRoot = resolve(destination);
  const stagedRoot = join(
    dirname(destinationRoot),
    `.${basename(destinationRoot)}.tmp-${randomUUID()}`,
  );
  const sourceRootPath = sourceRoot ? resolve(sourceRoot) : undefined;
  const temporaryCheckout = sourceRootPath
    ? undefined
    : mkdtempSync(join(tmpdir(), 'riffra-sonalloy-'));

  try {
    mkdirSync(dirname(destinationRoot), { recursive: true });
    if (sourceRootPath) {
      if (!existsSync(sourceRootPath)) {
        throw new Error(`Sonalloy source directory is missing: ${sourceRootPath}`);
      }
    } else {
      runGit(['init', '-q', temporaryCheckout]);
      runGit(['remote', 'add', 'origin', sonalloyRepository], temporaryCheckout);
      runGit(
        ['fetch', '--quiet', '--depth', '1', 'origin', normalizedReleaseTag],
        temporaryCheckout,
      );
      runGit(['checkout', '--quiet', '--detach', 'FETCH_HEAD'], temporaryCheckout);
    }

    const resolvedSourceRoot = sourceRootPath ?? temporaryCheckout;

    const stagedBuiltinRoot = join(stagedRoot, 'instruments', 'builtin');
    stagePresets(join(resolvedSourceRoot, 'presets'), stagedBuiltinRoot, normalizedReleaseTag);

    for (const licenseFile of licenseFiles) {
      const source = join(resolvedSourceRoot, licenseFile);
      if (!existsSync(source)) {
        throw new Error(`Sonalloy release is missing ${licenseFile}: ${source}`);
      }
      mkdirSync(stagedRoot, { recursive: true });
      copyFileSync(source, join(stagedRoot, licenseFile));
    }

    replaceDirectory(destinationRoot, stagedRoot);
  } finally {
    if (existsSync(stagedRoot)) {
      rmSync(stagedRoot, { force: true, recursive: true });
    }
    if (temporaryCheckout && existsSync(temporaryCheckout)) {
      rmSync(temporaryCheckout, { force: true, recursive: true });
    }
  }
}

function parseOptions(args) {
  if (args.length < 2 || args.length % 2 !== 0) {
    throw new Error(usage);
  }

  const options = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (!['--destination', '--source-root', '--release-tag'].includes(option) || !value) {
      throw new Error(usage);
    }
    if (options.has(option)) {
      throw new Error(`Duplicate option: ${option}`);
    }
    options.set(option, value);
  }

  const destination = options.get('--destination');
  if (!destination) {
    throw new Error(usage);
  }

  return {
    destination,
    releaseTag: options.get('--release-tag'),
    sourceRoot: options.get('--source-root'),
  };
}

async function main() {
  const options = parseOptions(process.argv.slice(2));
  const releaseTag = options.releaseTag
    ? options.releaseTag.startsWith('v')
      ? options.releaseTag
      : `v${options.releaseTag}`
    : readSonalloyReleaseTag();
  console.log(`Preparing built-in resources from Sonalloy ${releaseTag}...`);
  prepareBuiltinResources({ ...options, releaseTag });
  console.log(`Built-in resources prepared at ${resolve(options.destination)}`);
}

const isMainModule =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (isMainModule) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
