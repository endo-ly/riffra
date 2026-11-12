import type { SessionState } from './store';

export const selectSession = (state: SessionState) => state.project.boot?.canonical.session ?? null;
export const selectProjectState = (state: SessionState) => state.project.boot?.projectState ?? null;
export const selectActiveProjectId = (state: SessionState) =>
  state.project.boot?.projectState.activeProjectId ?? null;
export const selectTrack = (state: SessionState, id: string | null) =>
  id === null ? null : (state.readModel?.tracksById.get(id) ?? null);
export const selectMidiClip = (state: SessionState, id: string | null) =>
  id === null ? null : (state.readModel?.midiClipsById.get(id) ?? null);
export const selectHostReady = (state: SessionState) =>
  state.host.connection.mode !== 'disconnected' && !state.host.switching;
export const selectStartupRestoreReady = (state: SessionState) =>
  selectHostReady(state) &&
  !!state.project.boot?.nativeAvailable &&
  !state.project.boot.safeMode &&
  state.runtime.startupFinished &&
  !state.jobs.active &&
  !state.jobs.activeJobId.current;
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
  const track = selectTrack(state, trackId);
  if (track?.kind !== 'instrument' || !track.instrument) return false;
  const source = track.instrument.source;
  return !(
    source.type === 'vst3' &&
    (source.disabledPlaceholder || selectMissingDeviceIds(state).includes(track.instrument.id))
  );
};

/** Position for views animated from the playhead ref while playback is running. */
export const selectStoppedPlayheadTick = (state: SessionState): number | null =>
  state.transport.status?.state === 'playing' ? null : state.transport.displayTick;
