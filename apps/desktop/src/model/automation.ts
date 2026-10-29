import type {
  Arrangement,
  AutomationInterpolation,
  AutomationPoint,
  AutomationTarget,
  CreativeSession,
  DeviceParameterInfo,
  EffectOwner,
  ProjectTimebase,
} from '@/model/generated';
import { ticksToSeconds } from '@/model/timebase';

export interface AutomationOption {
  target: AutomationTarget;
  name: string;
  staticValue?: number;
  min: number;
  max: number;
  unit: string;
  discrete?: boolean;
  steps?: number;
}

export function automationTargetKey(target: AutomationTarget): string {
  switch (target.kind) {
    case 'trackMix':
      return `track:${target.trackId}:${target.parameter}`;
    case 'busMix':
      return `bus:${target.busId}:${target.parameter}`;
    case 'masterGain':
      return 'master';
    case 'sendLevel':
      return `send:${target.sendId}`;
    case 'sidechainSendLevel':
      return `sidechain:${target.sidechainSendId}`;
    case 'deviceParameter':
      return JSON.stringify([target.deviceId, target.parameterId]);
  }
}

export function automationOwner(
  arrangement: Arrangement,
  target: AutomationTarget,
): EffectOwner | null {
  switch (target.kind) {
    case 'trackMix':
      return { kind: 'track', id: target.trackId };
    case 'busMix':
      return { kind: 'bus', id: target.busId };
    case 'masterGain':
      return { kind: 'master' };
    case 'sendLevel': {
      const send = arrangement.mixer.sends.find((send) => send.id === target.sendId);
      return send ? { kind: 'track', id: send.trackId } : null;
    }
    case 'sidechainSendLevel': {
      const send = arrangement.mixer.sidechainSends.find(
        (send) => send.id === target.sidechainSendId,
      );
      return send ? { kind: 'track', id: send.sourceTrackId } : null;
    }
    case 'deviceParameter': {
      const track = arrangement.tracks.find(
        (track) =>
          track.instrument?.id === target.deviceId ||
          track.effects.some((effect) => effect.id === target.deviceId),
      );
      if (track) return { kind: 'track', id: track.id };
      const bus = arrangement.mixer.buses.find((bus) =>
        bus.effects.some((effect) => effect.id === target.deviceId),
      );
      if (bus) return { kind: 'bus', id: bus.id };
      return arrangement.mixer.masterEffects.some((effect) => effect.id === target.deviceId)
        ? { kind: 'master' }
        : null;
    }
  }
}

export function automationDevices(session: CreativeSession, owner: EffectOwner) {
  const arrangement = session.arrangement;
  const track =
    owner.kind === 'track' ? arrangement.tracks.find((track) => track.id === owner.id) : undefined;
  const effects =
    owner.kind === 'master'
      ? arrangement.mixer.masterEffects
      : owner.kind === 'bus'
        ? (arrangement.mixer.buses.find((bus) => bus.id === owner.id)?.effects ?? [])
        : (track?.effects ?? []);
  const devices = effects.map((effect) => ({
    id: effect.id,
    name: effect.name,
    plugin: effect.plugin,
  }));
  if (track?.instrument?.source.type === 'vst3')
    devices.unshift({
      id: track.instrument.id,
      name: track.instrument.name,
      plugin: track.instrument.source,
    });
  return devices;
}

export function sidechainTargetName(
  arrangement: Arrangement,
  deviceId: string,
): string | undefined {
  for (const track of arrangement.tracks) {
    const effect = track.effects.find((effect) => effect.id === deviceId);
    if (effect) return effect.name;
  }
  for (const bus of arrangement.mixer.buses) {
    const effect = bus.effects.find((effect) => effect.id === deviceId);
    if (effect) return effect.name;
  }
  return arrangement.mixer.masterEffects.find((effect) => effect.id === deviceId)?.name;
}

export function automationOptions(
  session: CreativeSession,
  owner: EffectOwner,
  parameters: Readonly<Record<string, DeviceParameterInfo[]>>,
): AutomationOption[] {
  const { arrangement } = session;
  const options: AutomationOption[] = [];
  if (owner.kind === 'track') {
    const track = arrangement.tracks.find((track) => track.id === owner.id);
    if (track) {
      options.push({
        target: { kind: 'trackMix', trackId: track.id, parameter: 'volume' },
        name: `${track.name} · Volume`,
        staticValue: track.gainDb,
        min: -90,
        max: 24,
        unit: 'dB',
      });
      options.push({
        target: { kind: 'trackMix', trackId: track.id, parameter: 'pan' },
        name: `${track.name} · Pan`,
        staticValue: track.pan,
        min: -1,
        max: 1,
        unit: '',
      });
      for (const send of arrangement.mixer.sends.filter((send) => send.trackId === track.id)) {
        const destination = arrangement.mixer.buses.find((bus) => bus.id === send.returnBusId);
        options.push({
          target: { kind: 'sendLevel', sendId: send.id },
          name: `Send · ${destination?.name ?? send.returnBusId}`,
          staticValue: send.levelDb,
          min: -90,
          max: 24,
          unit: 'dB',
        });
      }
      for (const send of arrangement.mixer.sidechainSends.filter(
        (send) => send.sourceTrackId === track.id,
      )) {
        const deviceName = sidechainTargetName(arrangement, send.targetDeviceId);
        options.push({
          target: { kind: 'sidechainSendLevel', sidechainSendId: send.id },
          name: `Sidechain · ${deviceName ?? send.targetDeviceId}`,
          staticValue: send.levelDb,
          min: -90,
          max: 24,
          unit: 'dB',
        });
      }
    }
  } else if (owner.kind === 'bus') {
    const bus = arrangement.mixer.buses.find((bus) => bus.id === owner.id);
    if (bus) {
      options.push({
        target: { kind: 'busMix', busId: bus.id, parameter: 'gain' },
        name: `${bus.name} · Gain`,
        staticValue: bus.gainDb,
        min: -90,
        max: 24,
        unit: 'dB',
      });
      options.push({
        target: { kind: 'busMix', busId: bus.id, parameter: 'pan' },
        name: `${bus.name} · Pan`,
        staticValue: bus.pan,
        min: -1,
        max: 1,
        unit: '',
      });
    }
  } else {
    options.push({
      target: { kind: 'masterGain' },
      name: 'Master · Gain',
      staticValue: session.settings.masterDb,
      min: -90,
      max: 0,
      unit: 'dB',
    });
  }
  for (const device of automationDevices(session, owner)) {
    for (const parameter of parameters[device.id] ?? []) {
      if (!parameter.automatable || !parameter.parameterId) continue;
      options.push({
        target: {
          kind: 'deviceParameter',
          deviceId: device.id,
          parameterId: parameter.parameterId,
        },
        name: `${device.name} · ${parameter.name ?? parameter.parameterId}`,
        staticValue: device.plugin.parameterValues[parameter.index],
        min: 0,
        max: 1,
        unit: 'normalized',
        discrete: parameter.discrete,
        steps: parameter.stepCount,
      });
    }
  }
  return options;
}

export function automationValueAt(
  points: readonly AutomationPoint[],
  interpolation: AutomationInterpolation,
  tick: number,
  fallback: number,
  timebase: ProjectTimebase,
): number {
  if (points.length === 0) return fallback;
  if (tick <= points[0].tick) return points[0].value;
  if (typeof interpolation === 'object') {
    const duration = interpolation.linearAmplitude.durationSeconds;
    const seconds = ticksToSeconds(tick, timebase);
    let start = 10 ** (points[0].value / 20);
    let target = start;
    let boundary = ticksToSeconds(points[0].tick, timebase) - duration;
    for (const point of points.slice(1)) {
      const nextBoundary = ticksToSeconds(point.tick, timebase);
      if (seconds < nextBoundary) break;
      start = target;
      target = 10 ** (point.value / 20);
      boundary = nextBoundary;
    }
    const amplitude =
      start + (target - start) * Math.min(1, Math.max(0, (seconds - boundary) / duration));
    return amplitude > 0 ? 20 * Math.log10(amplitude) : -1000;
  }

  for (let index = 1; index < points.length; index += 1) {
    const right = points[index];
    if (tick >= right.tick) continue;
    const left = points[index - 1];
    if (interpolation === 'step') return left.value;
    const amount = (tick - left.tick) / (right.tick - left.tick);
    return left.value + (right.value - left.value) * amount;
  }
  return points.at(-1)!.value;
}
