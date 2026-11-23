import type { Rig, Track } from '@/model/generated';

/** Label of one physical input channel, e.g. `In 1`. */
export function inputLabel(index: number): string {
  return `In ${index + 1}`;
}

/** Input channels a rig can choose: the device's channels and the rig's own, in order. */
export function inputChoices(rig: Rig, deviceChannels: readonly number[]): number[] {
  return Array.from(new Set([rig.audioInput.channelIndex, ...deviceChannels])).sort(
    (left, right) => left - right,
  );
}

/** The first `Rig N` name no rig uses yet. */
export function nextRigName(rigs: readonly Rig[]): string {
  const names = new Set(rigs.map((rig) => rig.name));
  let number = rigs.length + 1;
  while (names.has(`Rig ${number}`)) number += 1;
  return `Rig ${number}`;
}

/** The track that monitors the same input as the rig, if any. */
export function monitoringTrack(tracks: readonly Track[], rig: Rig): Track | undefined {
  return tracks.find(
    (track) =>
      track.kind === 'audio' &&
      track.audioInput?.channelIndex === rig.audioInput.channelIndex &&
      (track.monitoring === 'on' || (track.monitoring === 'auto' && track.armed)),
  );
}

/** The rig's effect ids with one effect moved by a signed offset. */
export function movedEffectIds(rig: Rig, index: number, offset: number): string[] {
  const ids = rig.effects.map((effect) => effect.id);
  [ids[index], ids[index + offset]] = [ids[index + offset], ids[index]];
  return ids;
}
