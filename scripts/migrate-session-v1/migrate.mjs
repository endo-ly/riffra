#!/usr/bin/env node
// Converts every Project session in a DataRoot to session document version 1.
//
//   node scripts/migrate-session-v1/migrate.mjs --source <DataRoot> --target <new DataRoot>
//
// The source DataRoot is copied to the target first; only the copy is
// rewritten. Only projects/*/session.json is converted. Generations and .riffra
// packages are left as they are.

import fs from 'node:fs';
import path from 'node:path';
import { parseArgs } from 'node:util';

const SCHEMA_VERSION = 1;

const DEFAULT_ARRANGEMENT = {
  revision: 0,
  timebase: { ppq: 960, bpm: 120.0, timeSignatureNumerator: 4, timeSignatureDenominator: 4 },
  loopRange: { enabled: false, startTick: 0, endTick: 0 },
  tracks: [],
  audioClips: [],
  midiClips: [],
  automationLanes: [],
  markers: [],
  regions: [],
  harmonyEvents: [],
  recordingSessions: [],
  recordingPasses: [],
  takes: [],
};

// Per type: keys that were always required, and keys that the previous
// format filled with a default when absent (with that default).
const SCHEMA = {
  CreativeSession: {
    required: ['sessionId', 'updatedAtMs', 'settings'],
    defaults: { arrangement: DEFAULT_ARRANGEMENT },
  },
  SessionSettings: {
    required: ['masterDb'],
    defaults: { loopEnabled: false, countInBeats: 0, metronomeEnabled: false, note: '' },
  },
  Arrangement: {
    required: ['revision', 'timebase', 'loopRange', 'tracks', 'audioClips', 'midiClips'],
    defaults: {
      automationLanes: [],
      markers: [],
      regions: [],
      harmonyEvents: [],
      recordingSessions: [],
      recordingPasses: [],
      takes: [],
    },
  },
  Track: {
    required: ['id', 'name', 'kind', 'rack'],
    defaults: {
      gainDb: 0.0,
      pan: 0.0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
    },
  },
  TrackInstrument: { required: ['id', 'name', 'bypassed', 'source'], defaults: {} },
  Vst3Plugin: {
    required: ['path'],
    defaults: { parameterValues: [], disabledPlaceholder: false },
  },
  RackInstance: { required: ['devices'], defaults: { macros: [] } },
  RackDevice: {
    required: ['id', 'name', 'kind', 'bypassed', 'gainDb'],
    defaults: { parameterValues: [], disabledPlaceholder: false },
  },
  AudioClip: {
    required: [
      'id',
      'trackId',
      'assetId',
      'startTick',
      'sourceRange',
      'sourceSampleRate',
      'timelineDuration',
      'gainDb',
      'pan',
      'fadeIn',
      'fadeOut',
      'loopEnabled',
      'muted',
      'name',
    ],
    defaults: { fadeShape: 'equalPower', takeVariant: 'raw' },
  },
  MidiClip: {
    required: ['id', 'name', 'trackId', 'startTick', 'durationTicks'],
    defaults: { notes: [], events: [], muted: false, loopEnabled: false },
  },
  RecordingSessionRecord: {
    required: ['id', 'startTick'],
    defaults: { trackSlots: [], passIds: [] },
  },
  RecordingPassRecord: {
    required: ['id', 'sessionId', 'ordinal', 'startTick', 'durationTicks'],
    defaults: { partialStart: false, partialEnd: false, trackTakeIds: [] },
  },
  RecordingTakeRecord: {
    required: ['id', 'sessionId', 'trackId', 'startTick', 'durationTicks'],
    defaults: { passId: '', sourceStartSample: 0, sourceEndSample: 0 },
  },
  TakeAudioSource: {
    required: ['assetId', 'sourceStartSample', 'sourceEndSample'],
    defaults: { tailEndSample: 0, sampleRate: 0 },
  },
};

class MigrationAbort extends Error {}

function complete(type, value, at) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new MigrationAbort(`${at} is not a ${type} object`);
  }
  const { required, defaults } = SCHEMA[type];
  for (const [key, fallback] of Object.entries(defaults)) {
    if (!(key in value)) value[key] = structuredClone(fallback);
  }
  for (const key of required) {
    if (!(key in value)) throw new MigrationAbort(`${at}.${key} is missing`);
  }
  return value;
}

function completeEach(type, values, at) {
  if (!Array.isArray(values)) throw new MigrationAbort(`${at} is not an array`);
  values.forEach((value, index) => complete(type, value, `${at}[${index}]`));
}

function effectFromRackDevice(device, at) {
  complete('RackDevice', device, at);
  if (device.kind !== 'plugin') {
    throw new MigrationAbort(`${at} is a "${device.kind}" device, which has no effect equivalent`);
  }
  if (typeof device.path !== 'string' || device.path.trim() === '') {
    throw new MigrationAbort(`${at} has no plugin path`);
  }
  const plugin = {
    path: device.path,
    parameterValues: device.parameterValues,
    disabledPlaceholder: device.disabledPlaceholder,
  };
  if (device.stateData !== undefined && device.stateData !== null) {
    plugin.stateData = device.stateData;
  }
  return { id: device.id, name: device.name, bypassed: device.bypassed, plugin };
}

function migrateTrack(track, at) {
  complete('Track', track, at);
  const rack = complete('RackInstance', track.rack, `${at}.rack`);
  if (rack.macros.length > 0) {
    throw new MigrationAbort(
      `${at}.rack.macros has ${rack.macros.length} macro(s), which would be lost`,
    );
  }
  if (!Array.isArray(rack.devices)) throw new MigrationAbort(`${at}.rack.devices is not an array`);
  track.effects = rack.devices.map((device, index) =>
    effectFromRackDevice(device, `${at}.rack.devices[${index}]`),
  );
  delete track.rack;
  if (track.instrument !== undefined && track.instrument !== null) {
    const instrument = complete('TrackInstrument', track.instrument, `${at}.instrument`);
    if (instrument.source?.type === 'vst3') {
      complete('Vst3Plugin', instrument.source, `${at}.instrument.source`);
    }
  }
}

function migrateSession(session) {
  complete('CreativeSession', session, 'session');
  complete('SessionSettings', session.settings, 'session.settings');
  const arrangement = complete('Arrangement', session.arrangement, 'session.arrangement');
  const at = 'session.arrangement';
  if (!Array.isArray(arrangement.tracks)) throw new MigrationAbort(`${at}.tracks is not an array`);
  arrangement.tracks.forEach((track, index) => migrateTrack(track, `${at}.tracks[${index}]`));
  completeEach('AudioClip', arrangement.audioClips, `${at}.audioClips`);
  completeEach('MidiClip', arrangement.midiClips, `${at}.midiClips`);
  completeEach('RecordingSessionRecord', arrangement.recordingSessions, `${at}.recordingSessions`);
  completeEach('RecordingPassRecord', arrangement.recordingPasses, `${at}.recordingPasses`);
  completeEach('RecordingTakeRecord', arrangement.takes, `${at}.takes`);
  arrangement.takes.forEach((take, index) => {
    for (const key of ['rawAudio', 'processedAudio']) {
      if (take[key] !== undefined && take[key] !== null) {
        complete('TakeAudioSource', take[key], `${at}.takes[${index}].${key}`);
      }
    }
  });
  return { schemaVersion: SCHEMA_VERSION, session };
}

function migrateProject(projectDir) {
  const file = path.join(projectDir, 'session.json');
  if (!fs.existsSync(file)) return { status: 'aborted', reason: 'session.json is missing' };
  let value;
  try {
    value = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (error) {
    return { status: 'aborted', reason: `session.json is not valid JSON: ${error.message}` };
  }
  if (value !== null && typeof value === 'object' && 'schemaVersion' in value) {
    return { status: 'skipped', reason: `already has schemaVersion ${value.schemaVersion}` };
  }
  try {
    const document = migrateSession(value);
    fs.writeFileSync(file, `${JSON.stringify(document, null, 2)}\n`);
    return { status: 'converted' };
  } catch (error) {
    if (error instanceof MigrationAbort) return { status: 'aborted', reason: error.message };
    throw error;
  }
}

function main() {
  const { values } = parseArgs({
    options: { source: { type: 'string' }, target: { type: 'string' } },
  });
  if (!values.source || !values.target) {
    console.error('usage: node migrate.mjs --source <DataRoot> --target <new DataRoot>');
    return 2;
  }
  const source = path.resolve(values.source);
  const target = path.resolve(values.target);
  if (!fs.existsSync(path.join(source, 'projects'))) {
    console.error(`${source} is not a DataRoot: projects/ is missing`);
    return 2;
  }
  if (fs.existsSync(target)) {
    console.error(`${target} already exists; choose a new directory`);
    return 2;
  }
  fs.cpSync(source, target, { recursive: true, errorOnExist: true });

  const projectsDir = path.join(target, 'projects');
  const projects = fs
    .readdirSync(projectsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  let aborted = 0;
  for (const projectId of projects) {
    const result = migrateProject(path.join(projectsDir, projectId));
    if (result.status === 'aborted') aborted += 1;
    console.log([projectId, result.status, result.reason].filter(Boolean).join('\t'));
  }
  console.log(`${projects.length} project(s), ${aborted} aborted; target: ${target}`);
  return aborted === 0 ? 0 : 1;
}

process.exitCode = main();
