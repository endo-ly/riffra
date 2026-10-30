import type { SessionState } from './store';

export const selectSession = (state: SessionState) => state.project.boot?.canonical.session ?? null;
export const selectProjectState = (state: SessionState) => state.project.boot?.projectState ?? null;
export const selectActiveProjectId = (state: SessionState) =>
  state.project.boot?.projectState.activeProjectId ?? null;
export const selectHostReady = (state: SessionState) =>
  state.host.connection.mode !== 'disconnected' && !state.host.switching;
export const selectPlaybackOutOfSync = (state: SessionState) =>
  state.runtime.projection.state === 'failed' &&
  state.runtime.projection.lastErrorCode !== 'timelineBusy';
export const selectRuntimeReady = (state: SessionState) =>
  !selectPlaybackOutOfSync(state) &&
  !['starting', 'faulted', 'offline'].includes(state.audio.status.state);
export const selectMissingDeviceIds = (state: SessionState) => [
  ...new Set([
    ...state.missing.items.filter((item) => item.kind === 'plugin').map((item) => item.id),
    ...(state.runtime.projection.activeDiagnostics?.missingDeviceIds ?? []),
  ]),
];
export const selectTrackInstrumentPlayable = (state: SessionState, trackId: string | null) => {
  const track =
    selectSession(state)?.arrangement.tracks.find((item) => item.id === trackId) ?? null;
  if (track?.kind !== 'instrument' || !track.instrument) return false;
  const source = track.instrument.source;
  return !(
    source.type === 'vst3' &&
    (source.disabledPlaceholder || selectMissingDeviceIds(state).includes(track.instrument.id))
  );
};
