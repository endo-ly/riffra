import { describe, expect, it } from 'vitest';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import type { MidiClip, Track } from '@/model/generated';
import { createSessionStore } from '@/shared/session/store';
import {
  acceptActivation,
  acceptBootstrap,
  acceptCanonical,
  replaceProjectList,
  type ProjectStore,
} from '@/shared/session/slices/project';

describe('project store', () => {
  it.each(['project', 'session'] as const)(
    'shares bootstrap data only within the same %s scope',
    (scope) => {
      const api = createFakeNativeApi();
      const store = createSessionStore(api);
      store.getState().project.acceptBootstrap(structuredClone(api.bootstrapState));
      const original = store.getState();
      store.getState().workspace.setLowerHeight(420);

      store.getState().project.acceptBootstrap(structuredClone(api.bootstrapState));
      expect(store.getState().readModel).toBe(original.readModel);
      expect(store.getState().workspace.lower.heights.detail).toBe(420);

      const boot = structuredClone(store.getState().project.boot!);
      boot.canonical.sequence += 1;
      if (scope === 'project') {
        boot.canonical.projectId = 'other-project';
        boot.projectState.activeProjectId = 'other-project';
      } else boot.canonical.session.sessionId = 'other-session';
      store.getState().project.acceptActivation({
        canonical: boot.canonical,
        projectState: boot.projectState,
        recovery: boot.recovery,
      });

      expect(store.getState().readModel!.tracksById).not.toBe(original.readModel!.tracksById);
      expect(store.getState().readModel!.session.arrangement.timebase).not.toBe(
        original.readModel!.session.arrangement.timebase,
      );
      expect(store.getState().workspace.lower.heights.detail).toBe(380);
    },
  );

  it('preserves unchanged entities and indexes through editing, reordering, deletion and undo', () => {
    const api = createFakeNativeApi();
    const store = createSessionStore(api);
    const session = structuredClone(api.bootstrapState.canonical.session);
    const track = (id: string): Track => ({
      id,
      name: id,
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
      panLaw: 'equalPower',
      externalAudioSourceTrackIds: [],
    });
    const clip = (id: string): MidiClip => ({
      id,
      trackId: id,
      name: id,
      startTick: 0,
      durationTicks: 960,
      muted: false,
      loopEnabled: false,
      notes: [
        {
          id: `note:${id}`,
          note: 60,
          startTick: 0,
          durationTicks: 120,
          velocity: 80,
          channel: 0,
          lyric: 'la',
        },
      ],
      events: [],
      instrumentControlEvents: [],
    });
    session.arrangement.tracks = [track('a'), track('b')];
    session.arrangement.midiClips = [clip('a'), clip('b')];
    store.getState().project.acceptBootstrap({
      ...api.bootstrapState,
      canonical: { ...api.bootstrapState.canonical, session },
    });
    const original = store.getState();
    const incoming = structuredClone(session);
    incoming.arrangement.revision += 1;
    incoming.arrangement.midiClips[0].notes[0].note = 64;
    delete incoming.arrangement.midiClips[0].notes[0].lyric;

    store.getState().project.acceptCanonical(api.mutationResult(incoming).canonical);

    const edited = store.getState();
    const accepted = edited.project.boot!.canonical.session;
    expect(accepted).toStrictEqual(incoming);
    expect(session.arrangement.midiClips[0].notes[0].note).toBe(60);
    expect(accepted.arrangement.timebase).toBe(session.arrangement.timebase);
    expect(accepted.arrangement.tracks).toBe(session.arrangement.tracks);
    expect(accepted.arrangement.midiClips[1]).toBe(session.arrangement.midiClips[1]);
    expect(edited.workspace).toBe(original.workspace);
    expect(edited.readModel!.tracksById).toBe(original.readModel!.tracksById);
    expect(edited.readModel!.clipsByTrack.get('b')).toBe(original.readModel!.clipsByTrack.get('b'));
    expect(edited.readModel!.midiClipsById.get('a')).toBe(accepted.arrangement.midiClips[0]);
    expect(edited.readModel!.dependencyKey).toBe(original.readModel!.dependencyKey);

    const reordered = structuredClone(accepted);
    reordered.arrangement.tracks.reverse();
    reordered.arrangement.midiClips.reverse();
    store.getState().project.acceptCanonical(api.mutationResult(reordered).canonical);
    expect(store.getState().project.boot!.canonical.session).toStrictEqual(reordered);
    expect(store.getState().project.boot!.canonical.session.arrangement.midiClips[0]).toBe(
      session.arrangement.midiClips[1],
    );
    expect(store.getState().project.boot!.canonical.session.arrangement.midiClips[1]).toBe(
      accepted.arrangement.midiClips[0],
    );

    const deleted = structuredClone(reordered);
    deleted.arrangement.midiClips.splice(0, 1);
    store.getState().workspace.openMidiEditor('b');
    store.getState().project.acceptCanonical(api.mutationResult(deleted).canonical);
    expect(store.getState().readModel!.midiClipsById.has('b')).toBe(false);
    expect(store.getState().readModel!.clipsByTrack.has('b')).toBe(false);
    expect(store.getState().workspace.lower.view).toBe('closed');

    store
      .getState()
      .project.acceptCanonical(api.mutationResult(structuredClone(session)).canonical);
    expect(store.getState().project.boot!.canonical.session).toStrictEqual(session);
    expect(store.getState().readModel!.midiClipsById.get('b')).toBeDefined();
    expect(store.getState().readModel!.midiClipsById.get('a')!.notes[0].lyric).toBe('la');
  });

  it('accepts only newer canonical state for the active project', () => {
    const boot = createFakeNativeApi().bootstrapState;
    const store = acceptBootstrap({ boot: null, lastActivation: null }, boot);
    expect(acceptCanonical(store, boot.canonical).result).toBe('already-current');
    expect(
      acceptCanonical(store, { ...boot.canonical, projectId: 'other', sequence: 99 }).result,
    ).toBe('rejected');
    expect(acceptCanonical({ boot: null, lastActivation: null }, boot.canonical).result).toBe(
      'rejected',
    );
    expect(
      acceptCanonical(store, { ...boot.canonical, sequence: boot.canonical.sequence - 1 }).result,
    ).toBe('rejected');
    const newer = { ...boot.canonical, sequence: boot.canonical.sequence + 1 };
    expect(acceptCanonical(store, newer).store.boot?.canonical).toEqual(newer);
    expect(acceptBootstrap(acceptCanonical(store, newer).store, boot).boot?.canonical).toEqual(
      newer,
    );
  });

  it('preserves the activation when its response and an older list arrive later', () => {
    const boot = createFakeNativeApi().bootstrapState;
    const store = acceptBootstrap({ boot: null, lastActivation: null }, boot);
    const projectId = 'project:new';
    const activation = {
      canonical: { ...boot.canonical, projectId, sequence: boot.canonical.sequence + 1 },
      projectState: { ...boot.projectState, activeProjectId: projectId },
      recovery: boot.recovery,
    };
    const active = acceptActivation(store, activation) as ProjectStore;
    expect(acceptActivation(active, activation)).toBeNull();
    const listed = replaceProjectList(active, boot.projectState);
    expect(listed.boot?.projectState.activeProjectId).toBe(projectId);
    expect(listed.boot?.canonical).toEqual(activation.canonical);
    expect(
      acceptCanonical(listed, { ...boot.canonical, sequence: activation.canonical.sequence + 1 })
        .result,
    ).toBe('rejected');
  });
});
