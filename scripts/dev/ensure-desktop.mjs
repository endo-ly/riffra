import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { prepareBuiltinResources, readSonalloyReleaseTag } from '../resources/prepare-builtin.mjs';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const desktopRoot = join(repositoryRoot, 'apps/desktop/src-tauri');
const binariesRoot = join(desktopRoot, 'binaries');
const resourcesRoot = join(desktopRoot, 'resources');
const runtimeStampPath = join(binariesRoot, '.riffra-desktop-runtime.json');
const nativeBuildConfiguration = 'Debug';
const sidecarNames = ['riffra-audio', 'riffra-plugin-scan', 'riffra-render'];

function getRustHostTriple() {
  try {
    const output = execFileSync('rustc', ['-vV'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
    });
    const hostTriple = output.match(/^host:\s*(\S+)$/m)?.[1];
    if (hostTriple) {
      return hostTriple;
    }
  } catch {
    // The command below provides the actionable error for the user.
  }

  throw new Error('Could not determine the Rust host target. Make sure rustc is installed.');
}

function sidecarPaths(targetTriple) {
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const packaged = sidecarNames.map((name) =>
    join(binariesRoot, `${name}-${targetTriple}${suffix}`),
  );
  const development = sidecarNames.map((name) =>
    join(repositoryRoot, 'target/debug', `${name}${suffix}`),
  );
  return { packaged, development, all: [...packaged, ...development] };
}

function isRunnableFile(path) {
  try {
    const stats = statSync(path);
    if (!stats.isFile()) {
      return false;
    }
    return process.platform === 'win32' || (stats.mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

function inspectResources(expectedRelease) {
  const requiredFiles = [
    join(resourcesRoot, 'instruments/builtin/manifest.json'),
    join(resourcesRoot, 'THIRD_PARTY_NOTICES.md'),
    join(resourcesRoot, 'LICENSE-MIT'),
    join(resourcesRoot, 'LICENSE-APACHE'),
  ];
  const missingFile = requiredFiles.find((path) => !existsSync(path) || !statSync(path).isFile());
  if (missingFile) {
    return {
      current: false,
      releaseMismatch: false,
      reason: `missing ${relative(repositoryRoot, missingFile)}`,
    };
  }

  try {
    const manifest = JSON.parse(
      readFileSync(join(resourcesRoot, 'instruments/builtin/manifest.json'), 'utf8'),
    );
    if (manifest.sourceRelease !== expectedRelease) {
      return {
        current: false,
        releaseMismatch: true,
        reason: `resource release is ${manifest.sourceRelease ?? 'unknown'}, expected ${expectedRelease}`,
      };
    }
  } catch {
    return {
      current: false,
      releaseMismatch: false,
      reason: 'built-in resource manifest is invalid',
    };
  }

  return { current: true };
}

function nativeInputFiles() {
  const nativeRoot = join(repositoryRoot, 'native/audio-engine');
  const files = [];
  const includedTopLevelFiles = new Set(['CMakeLists.txt']);

  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      const pathFromNativeRoot = relative(nativeRoot, path).replaceAll('\\', '/');
      if (pathFromNativeRoot === 'build' || pathFromNativeRoot.startsWith('build/')) {
        continue;
      }

      if (entry.isDirectory()) {
        visit(path);
        continue;
      }

      const topLevel = !pathFromNativeRoot.includes('/');
      if (
        (topLevel && includedTopLevelFiles.has(pathFromNativeRoot)) ||
        pathFromNativeRoot.startsWith('cmake/') ||
        pathFromNativeRoot.startsWith('src/')
      ) {
        files.push(path);
      }
    }
  }

  visit(nativeRoot);
  return files.sort();
}

function nativeInputFingerprint() {
  const hash = createHash('sha256');
  const nativeRoot = join(repositoryRoot, 'native/audio-engine');
  for (const path of nativeInputFiles()) {
    hash.update(relative(nativeRoot, path).replaceAll('\\', '/'));
    hash.update('\0');
    hash.update(readFileSync(path));
  }
  return hash.digest('hex');
}

function readRuntimeStamp() {
  if (!existsSync(runtimeStampPath)) {
    return undefined;
  }

  try {
    return JSON.parse(readFileSync(runtimeStampPath, 'utf8'));
  } catch {
    return undefined;
  }
}

function inspectAudioProbe(executable) {
  try {
    const output = execFileSync(executable, ['--probe'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
      timeout: 10000,
      windowsHide: true,
    });
    const response = output
      .split(/\r?\n/)
      .map((line) => {
        try {
          return JSON.parse(line);
        } catch {
          return undefined;
        }
      })
      .find((value) => value?.type === 'audioDeviceProbe');

    if (!response) {
      return { current: false, reason: 'audio sidecar returned no probe response' };
    }
    if (
      !Array.isArray(response.drivers) ||
      !Number.isFinite(response.refreshedAtMs) ||
      typeof response.message !== 'string'
    ) {
      return { current: false, reason: 'audio sidecar probe response uses an old protocol' };
    }
  } catch {
    return { current: false, reason: 'audio sidecar probe failed' };
  }

  return { current: true };
}

function inspectSidecars({ targetTriple, expectedRelease, inputFingerprint }) {
  const paths = sidecarPaths(targetTriple);
  const missingSidecar = paths.all.find((path) => !isRunnableFile(path));
  if (missingSidecar) {
    return { current: false, reason: `missing ${relative(repositoryRoot, missingSidecar)}` };
  }

  const stamp = readRuntimeStamp();
  if (!stamp) {
    for (const audioSidecar of [paths.packaged[0], paths.development[0]]) {
      const probe = inspectAudioProbe(audioSidecar);
      if (!probe.current) {
        return {
          current: false,
          reason: `${relative(repositoryRoot, audioSidecar)}: ${probe.reason}`,
        };
      }
    }
    return { current: true, needsStamp: true };
  }

  if (
    stamp.targetTriple !== targetTriple ||
    stamp.configuration !== nativeBuildConfiguration ||
    stamp.sourceRelease !== expectedRelease ||
    stamp.inputFingerprint !== inputFingerprint
  ) {
    return { current: false, reason: 'native sidecars were built from a different runtime state' };
  }

  return { current: true };
}

function runNativeBuild() {
  const script = join(repositoryRoot, 'native/audio-engine');
  if (process.platform === 'win32') {
    execFileSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        join(script, 'build.ps1'),
        '-Configuration',
        nativeBuildConfiguration,
        '-SidecarsOnly',
        '-SkipTests',
      ],
      { cwd: repositoryRoot, stdio: 'inherit' },
    );
    return;
  }

  execFileSync('bash', [join(script, 'build.sh'), nativeBuildConfiguration], {
    cwd: repositoryRoot,
    env: { ...process.env, SIDECARS_ONLY: '1', SKIP_TESTS: '1' },
    stdio: 'inherit',
  });
}

function writeRuntimeStamp({ targetTriple, expectedRelease, inputFingerprint }) {
  writeFileSync(
    runtimeStampPath,
    `${JSON.stringify(
      {
        targetTriple,
        configuration: nativeBuildConfiguration,
        sourceRelease: expectedRelease,
        inputFingerprint,
      },
      null,
      2,
    )}\n`,
  );
}

function ensureSidecarsExist(targetTriple) {
  const missingSidecar = sidecarPaths(targetTriple).all.find((path) => !isRunnableFile(path));
  if (missingSidecar) {
    throw new Error(
      `Native build completed without installing ${relative(repositoryRoot, missingSidecar)}.`,
    );
  }
}

function ensureFinalResources(expectedRelease) {
  const resources = inspectResources(expectedRelease);
  if (!resources.current) {
    throw new Error(`Desktop resources are not ready: ${resources.reason}.`);
  }
}

function main() {
  const expectedRelease = readSonalloyReleaseTag(repositoryRoot);
  const targetTriple = getRustHostTriple();
  const inputFingerprint = nativeInputFingerprint();
  const resources = inspectResources(expectedRelease);
  const sidecars = inspectSidecars({ targetTriple, expectedRelease, inputFingerprint });
  const nativeBuildRequired = !sidecars.current || resources.releaseMismatch === true;

  if (resources.current && sidecars.current) {
    if (sidecars.needsStamp) {
      writeRuntimeStamp({ targetTriple, expectedRelease, inputFingerprint });
    }
    console.log('Desktop runtime is ready.');
    return;
  }

  if (!resources.current && !nativeBuildRequired) {
    console.log(`Preparing desktop resources (${resources.reason})...`);
    prepareBuiltinResources({ destination: resourcesRoot, releaseTag: expectedRelease });
  }

  if (nativeBuildRequired) {
    console.log(`Building native sidecars (${sidecars.reason}; tests skipped)...`);
    runNativeBuild();
    ensureSidecarsExist(targetTriple);
  }

  ensureFinalResources(expectedRelease);
  if (nativeBuildRequired || sidecars.needsStamp) {
    writeRuntimeStamp({ targetTriple, expectedRelease, inputFingerprint });
  }
  console.log('Desktop runtime is ready.');
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
