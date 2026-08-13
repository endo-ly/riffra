#!/usr/bin/env node
// Cross-platform verification entry point for Riffra.
//
// Usage:
//   node scripts/verify.mjs
//   node scripts/verify.mjs --native

import { spawnSync } from 'node:child_process';
import { platform } from 'node:os';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

process.env.NODE_NO_WARNINGS = '1';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');
const isWin = platform() === 'win32';

function run(label, command, args = [], options = {}) {
  console.log(`\n== ${label} ==`);

  let resolved = command;
  let finalArgs = args;

  // Node.js on Windows refuses to spawn .cmd files directly (EINVAL);
  // route them through cmd.exe /c. Arguments here are controlled by the
  // script, not arbitrary user input, so this is safe.
  if (isWin && resolved.toLowerCase().endsWith('.cmd')) {
    finalArgs = ['/c', resolved, ...args];
    resolved = 'cmd.exe';
  }

  const result = spawnSync(resolved, finalArgs, {
    stdio: 'inherit',
    cwd: root,
    ...options,
  });
  if (result.error) {
    throw new Error(`${label}: failed to start ${command}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${label} failed with exit code ${result.status}`);
  }
  if (result.signal) {
    throw new Error(`${label} terminated by signal ${result.signal}`);
  }
}

function runSilent(command, args = [], options = {}) {
  return spawnSync(command, args, { encoding: 'utf8', cwd: root, ...options });
}

function findOnPath(name) {
  const result = runSilent(isWin ? 'where' : 'which', [name]);
  if (result.status !== 0 || !result.stdout) return null;
  return result.stdout.trim().split('\n')[0].trim();
}

function resolveCommand(command) {
  if (!isWin || command.includes('\\') || command.includes('/')) return command;
  return (
    findOnPath(`${command}.cmd`) || findOnPath(`${command}.exe`) || findOnPath(command) || command
  );
}

function collectCppFiles() {
  const result = runSilent('git', ['ls-files', 'native/audio-engine']);
  if (result.status !== 0 || !result.stdout) return [];
  return result.stdout
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => /\.(cpp|h|hpp|cc|hh)$/i.test(line))
    .map((line) => resolve(root, line));
}

function snapshotDirectory(directory) {
  const files = [];
  const visit = (current) => {
    for (const entry of readdirSync(current)) {
      const path = join(current, entry);
      if (statSync(path).isDirectory()) visit(path);
      else files.push([relative(directory, path), readFileSync(path, 'utf8')]);
    }
  };
  visit(directory);
  return JSON.stringify(files.sort(([left], [right]) => left.localeCompare(right)));
}

function buildNative({ config = 'Debug', buildDir, withTests = false } = {}) {
  const engineDir = join(root, 'native', 'audio-engine');

  if (isWin) {
    const script = join(engineDir, 'build.ps1');
    const args = ['-Configuration', config];
    if (buildDir) args.push('-BuildDirectory', buildDir);
    if (!withTests) args.push('-SkipTests');
    run(
      'Build native audio engine',
      'powershell',
      ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, ...args],
      { cwd: engineDir },
    );
  } else {
    const script = join(engineDir, 'build.sh');
    const env = { ...process.env };
    if (buildDir) env.BUILD_DIR = buildDir;
    if (!withTests) env.SKIP_TESTS = '1';
    run('Build native audio engine', 'bash', [script, config], { env, cwd: engineDir });
  }
}

function findInstalledRenderWorker() {
  const directory = join(root, 'apps', 'desktop', 'src-tauri', 'binaries');
  const name = readdirSync(directory).find(
    (entry) => entry.startsWith('riffra-render-') && entry.endsWith(isWin ? '.exe' : ''),
  );
  if (!name) throw new Error('Installed riffra-render worker was not found.');
  return join(directory, name);
}

function main() {
  const native = process.argv.slice(2).includes('--native');
  const artifactsRoot = join(root, '.artifacts', 'verify');
  process.env.CARGO_TARGET_DIR = join(artifactsRoot, 'cargo');

  const generatedTypes = join(root, 'apps', 'desktop', 'src', 'model', 'generated');
  const typesBeforeGeneration = snapshotDirectory(generatedTypes);
  run('Regenerate TypeScript bindings', resolveCommand('npm'), ['run', 'gen:types']);
  if (snapshotDirectory(generatedTypes) !== typesBeforeGeneration) {
    throw new Error('TypeScript bindings were stale before verification');
  }
  run('TypeScript build and tests', resolveCommand('npm'), ['run', 'check']);
  run('ESLint', resolveCommand('npm'), ['run', 'lint']);
  run('Prettier check', resolveCommand('npm'), ['run', 'format:check']);
  run('Knip', resolveCommand('npx'), [
    'knip',
    '--tsConfig',
    'apps/desktop/tsconfig.app.json',
    '--include=files,dependencies',
    '--no-config-hints',
  ]);
  run('Rust formatting', 'cargo', ['fmt', '--manifest-path', 'Cargo.toml', '--all', '--check']);
  run('Rust clippy', 'cargo', [
    'clippy',
    '--manifest-path',
    'Cargo.toml',
    '--workspace',
    '--all-targets',
    '--',
    '-D',
    'warnings',
  ]);
  run('Rust tests', 'cargo', ['test', '--manifest-path', 'Cargo.toml', '--workspace']);

  if (native) {
    buildNative({
      config: 'Debug',
      buildDir: join(artifactsRoot, 'native'),
      withTests: true,
    });
    run(
      'Rust to native offline render',
      'cargo',
      [
        'test',
        '--manifest-path',
        'Cargo.toml',
        '-p',
        'riffra-render-worker',
        '--test',
        'native_worker',
        '--',
        '--ignored',
      ],
      {
        env: {
          ...process.env,
          RIFFRA_RENDER_WORKER: findInstalledRenderWorker(),
        },
      },
    );
  }

  const clangFormat = findOnPath('clang-format');
  if (clangFormat) {
    const cppFiles = collectCppFiles();
    if (cppFiles.length > 0) {
      run('C++ formatting', clangFormat, ['--dry-run', '--Werror', ...cppFiles]);
    }
  } else {
    console.log('\n== C++ formatting skipped: clang-format is not installed ==');
  }

  run('Git whitespace check', 'git', ['diff', '--check']);
  console.log('\nVerification completed successfully.');
}

main();
