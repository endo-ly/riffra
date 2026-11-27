import { clipDurationTicks } from '@/model/clip-time';
import { tickToMusicalPosition } from '@/model/time-format';
import { useSession, useSessionContext } from '@/session/session-context';
import { Icon } from '@/shared/ui/Icon';
import controls from '@/shared/ui/controls.module.css';
import { AudioClipInspector } from './AudioClipInspector';
import { MidiClipInspector } from './MidiClipInspector';
import { MultiClipInspector } from './MultiClipInspector';
import styles from './PropertiesPanel.module.css';
import { TakeInspector } from './TakeInspector';
import { TrackInspector } from './TrackInspector';

export function PropertiesPanel() {
  const { api } = useSessionContext();
  const session = useSession((state) => state.project.boot!.canonical.session);
  const workspace = useSession((state) => state.workspace);
  const audio = useSession((state) => state.audio.status);
  const recording = useSession((state) => state.recording);
  const editorSelection = workspace.selection;
  const selectedTrackId = editorSelection.kind === 'track' ? editorSelection.trackId : undefined;
  const selectedTrack = session.arrangement.tracks.find((track) => track.id === selectedTrackId);
  const selectedClipIds = editorSelection.kind === 'clips' ? editorSelection.clipIds : [];
  const selectedAudioClipIds = session.arrangement.audioClips
    .filter((clip) => selectedClipIds.includes(clip.id))
    .map((clip) => clip.id);
  const selectedMidiClipIds = session.arrangement.midiClips
    .filter((clip) => selectedClipIds.includes(clip.id))
    .map((clip) => clip.id);
  const selectedAudioClipCount = selectedAudioClipIds.length;
  const selectedMidiClipCount = selectedMidiClipIds.length;
  const recordingActive = audio.recording.active || audio.recording.processing;
  const recordingCommandPending = recording.commandPending || recording.stopPending;
  const recordAnotherTake = async (recordingSessionId: string) => {
    await recording.start(recordingSessionId);
  };
  const setSelectedClipIds = (clipIds: string[]) =>
    workspace.setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
  const takeInspector = (
    <TakeInspector
      session={session}
      selection={editorSelection}
      recordingActive={recordingActive}
      recordingCommandPending={recordingCommandPending}
      onRecordAnotherTake={recordAnotherTake}
    />
  );
  return (
    <aside className={styles.panel} aria-label="Properties" data-properties-panel>
      <header className={styles.header}>
        <strong>Inspector</strong>
        <button
          type="button"
          className={controls.iconButton}
          aria-label="Close Inspector"
          title="Close Inspector (Ctrl+I)"
          onClick={() => workspace.setInspectorOpen(false)}
        >
          <Icon name="close" />
        </button>
      </header>
      <div className={styles.body}>
        {selectedTrack ? (
          <>
            <TrackInspector
              track={selectedTrack}
              session={session}
              audio={audio}
              onOpenDevices={workspace.openDevices}
            />
            {takeInspector}
          </>
        ) : selectedAudioClipCount + selectedMidiClipCount > 1 ? (
          <MultiClipInspector
            session={session}
            selectedAudioClipIds={selectedAudioClipIds}
            selectedMidiClipIds={selectedMidiClipIds}
            setSelectedClipIds={setSelectedClipIds}
          />
        ) : selectedMidiClipCount === 1 ? (
          <>
            <MidiClipInspector
              session={session}
              selectedClipIds={selectedMidiClipIds}
              setSelectedClipIds={setSelectedClipIds}
            />
            {takeInspector}
          </>
        ) : selectedAudioClipCount === 1 ? (
          <>
            <AudioClipInspector
              session={session}
              selectedClipIds={selectedAudioClipIds}
              setSelectedClipIds={setSelectedClipIds}
              onSetLoopToClip={(clip) => {
                const timebase = session.arrangement.timebase;
                const endTicks = clipDurationTicks(clip, timebase);
                return api.updateTimelineLoopRange(
                  true,
                  tickToMusicalPosition(clip.startTick, timebase),
                  tickToMusicalPosition(clip.startTick + endTicks, timebase),
                );
              }}
            />
            {takeInspector}
          </>
        ) : (
          <div className={styles.empty}>
            <Icon name="pointer" />
            <strong>Nothing selected</strong>
            <small>Select a track or a clip in the arrangement to inspect it here.</small>
          </div>
        )}
      </div>
    </aside>
  );
}
