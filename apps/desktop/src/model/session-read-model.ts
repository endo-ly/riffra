import type {
  AudioClip,
  CreativeSession,
  EffectDevice,
  MidiClip,
  Track,
  Vst3Plugin,
} from './generated';

export interface TrackClips {
  readonly audio: readonly AudioClip[];
  readonly midi: readonly MidiClip[];
}

/** Read-only indexes derived from the accepted canonical session. */
export interface SessionReadModel {
  readonly session: CreativeSession;
  readonly tracksById: ReadonlyMap<string, Track>;
  readonly midiClipsById: ReadonlyMap<string, MidiClip>;
  readonly clipsByTrack: ReadonlyMap<string, TrackClips>;
  readonly dependencyKey: string;
}

function groupClips(
  audio: readonly AudioClip[],
  midi: readonly MidiClip[],
  previous?: ReadonlyMap<string, TrackClips>,
) {
  const grouped = new Map<string, { audio: AudioClip[]; midi: MidiClip[] }>();
  for (const clip of audio) {
    const group = grouped.get(clip.trackId) ?? { audio: [], midi: [] };
    group.audio.push(clip);
    grouped.set(clip.trackId, group);
  }
  for (const clip of midi) {
    const group = grouped.get(clip.trackId) ?? { audio: [], midi: [] };
    group.midi.push(clip);
    grouped.set(clip.trackId, group);
  }
  const result = new Map<string, TrackClips>();
  for (const [id, group] of grouped) {
    const old = previous?.get(id);
    const equal =
      old &&
      old.audio.length === group.audio.length &&
      old.midi.length === group.midi.length &&
      old.audio.every((clip, index) => clip === group.audio[index]) &&
      old.midi.every((clip, index) => clip === group.midi[index]);
    result.set(id, equal ? old : group);
  }
  return result;
}

// These are the inputs used by riffra-runtime's collect_missing, including its display metadata.
function dependencyKey(session: CreativeSession): string {
  const { arrangement } = session;
  const dependencies: unknown[] = arrangement.audioClips.map((clip) => [
    clip.id,
    clip.name,
    clip.assetId,
  ]);
  const plugin = (owner: string, id: string, name: string, source: Vst3Plugin) => {
    dependencies.push([owner, id, name, source.path, source.disabledPlaceholder]);
  };
  const effects = (owner: string, devices: readonly EffectDevice[]) => {
    for (const device of devices) plugin(owner, device.id, device.name, device.plugin);
  };
  for (const track of arrangement.tracks) {
    if (track.instrument?.source.type === 'vst3')
      plugin(
        `track:${track.id}:instrument`,
        track.instrument.id,
        track.instrument.name,
        track.instrument.source,
      );
    effects(`track:${track.id}`, track.effects);
  }
  for (const bus of arrangement.mixer.buses) effects(`bus:${bus.id}`, bus.effects);
  effects('master', arrangement.mixer.masterEffects);
  return JSON.stringify(dependencies);
}

/** Rebuilds only indexes whose source collections changed. */
export function buildSessionReadModel(
  previous: SessionReadModel | null,
  session: CreativeSession,
): SessionReadModel {
  if (previous?.session === session) return previous;
  const old = previous?.session.arrangement;
  const next = session.arrangement;
  const audioUnchanged = old?.audioClips === next.audioClips;
  const midiUnchanged = old?.midiClips === next.midiClips;
  return {
    session,
    tracksById:
      old?.tracks === next.tracks
        ? previous!.tracksById
        : new Map(next.tracks.map((track) => [track.id, track])),
    midiClipsById: midiUnchanged
      ? previous!.midiClipsById
      : new Map(next.midiClips.map((clip) => [clip.id, clip])),
    clipsByTrack:
      audioUnchanged && midiUnchanged
        ? previous!.clipsByTrack
        : groupClips(next.audioClips, next.midiClips, previous?.clipsByTrack),
    dependencyKey:
      audioUnchanged && old?.tracks === next.tracks && old?.mixer === next.mixer
        ? previous!.dependencyKey
        : dependencyKey(session),
  };
}
