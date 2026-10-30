import type { ReactNode } from 'react';
import type { CreativeSession } from '@/model/generated';
import { defaultSession } from '@/native/browser-defaults';
import { useSessionRuntime } from '@/app/runtime/useSessionRuntime';
import { useRuntimeProjectionToast } from '@/app/runtime/useRuntimeProjectionToast';
import { EditorWorkspace } from '@/app/workspace/EditorWorkspace';
import { ToastStack } from '@/shared/ui/ToastStack';
import { toAssetId } from '@/model/asset-id';
import type { FakeNativeApi } from '@/test/support/fake-native-api';
import { renderWithSession } from '@/test/support/session-provider';

function RuntimeBinding() {
  useSessionRuntime();
  return null;
}

function ProjectionToast() {
  useRuntimeProjectionToast();
  return null;
}

interface WorkspaceRenderOptions {
  session?: CreativeSession;
  focusedTrackId?: string | null;
}

/** Renders the Arrange workspace under a session store wired to the fake Host. */
export function renderWorkspace(api: FakeNativeApi, options: WorkspaceRenderOptions = {}) {
  return renderWithSession(
    (
      <>
        <RuntimeBinding />
        <ProjectionToast />
        <EditorWorkspace />
        <ToastStack />
      </>
    ) as ReactNode,
    {
      api,
      seed: {
        session: options.session ?? defaultSession(),
        focusedTrackId: options.focusedTrackId ?? null,
      },
    },
  );
}

/** Two Audio Tracks with one clip whose start sits between snap boundaries. */
export function offGridAudioMoveSession(): CreativeSession {
  const session = defaultSession();
  for (const [id, name] of [
    ['track:source', 'Source'],
    ['track:target', 'Target'],
  ]) {
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower',
      id,
      name,
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
  }
  session.arrangement.audioClips.push({
    id: 'clip:off-grid',
    name: 'Off-grid clip',
    trackId: 'track:source',
    assetId: toAssetId('asset:audio'),
    startTick: 1_100,
    sourceRange: { start: 0, end: 48_000 },
    sourceSampleRate: 48_000,
    timelineDuration: { frames: 48_000, sampleRate: 48_000 },
    gainDb: 0,
    pan: 0,
    fadeIn: { frames: 0, sampleRate: 48_000 },
    fadeOut: { frames: 0, sampleRate: 48_000 },
    loopEnabled: false,
    muted: false,
    fadeShape: 'equalPower',
    takeVariant: 'raw',
  });
  return session;
}
