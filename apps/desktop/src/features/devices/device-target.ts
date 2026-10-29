import type { Arrangement, EffectDevice, EffectOwner, Track } from '@/model/generated';

/** Presentation target for the shared device chain. */
export interface DeviceTarget {
  owner: EffectOwner;
  name: string;
  effects: EffectDevice[];
  track: Track | null;
}

export function resolveDeviceTarget(
  arrangement: Arrangement,
  owner: EffectOwner,
): DeviceTarget | null {
  if (owner.kind === 'master')
    return { owner, name: 'Master', effects: arrangement.mixer.masterEffects, track: null };
  if (owner.kind === 'bus') {
    const bus = arrangement.mixer.buses.find((item) => item.id === owner.id);
    return bus ? { owner, name: bus.name, effects: bus.effects, track: null } : null;
  }
  const track = arrangement.tracks.find((item) => item.id === owner.id);
  return track ? { owner, name: track.name, effects: track.effects, track } : null;
}
