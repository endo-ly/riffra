import type { EditorSelection } from '@/model/editor-selection';
import { clipDurationTicks } from '@/model/clip-time';
import type { AudioStatus, CreativeSession } from '@/model/generated';
import { tickToMusicalPosition } from '@/model/time-format';
import { useSessionContext } from '@/shared/session/session-context';
import { Icon } from '@/shared/ui/primitives';
import { AudioClipInspector } from './AudioClipInspector';
import { MidiClipInspector } from './MidiClipInspector';
import { MultiClipInspector } from './MultiClipInspector';
import styles from './PropertiesPanel.module.css';
import { TakeInspector } from './TakeInspector';
import { TrackInspector } from './TrackInspector';

interface PropertiesPanelProps {
  onOpenDevices: () => void;
  audio: AudioStatus;
  recordingCommandPending: boolean;
  session: CreativeSession;
  editorSelection: EditorSelection;
  setEditorSelection: (selection: EditorSelection) => void;
  onRecordAnotherTake: (recordingSessionId: string) => void | Promise<void>;
}

export function PropertiesPanel(props: PropertiesPanelProps) {
  const { api } = useSessionContext();
  const selectedTrackId =
    props.editorSelection.kind === 'track' ? props.editorSelection.trackId : undefined;
  const selectedTrack = props.session.arrangement.tracks.find(
    (track) => track.id === selectedTrackId,
  );
  const selectedClipIds =
    props.editorSelection.kind === 'clips' ? props.editorSelection.clipIds : [];
  const selectedAudioClipIds = props.session.arrangement.audioClips
    .filter((clip) => selectedClipIds.includes(clip.id))
    .map((clip) => clip.id);
  const selectedMidiClipIds = props.session.arrangement.midiClips
    .filter((clip) => selectedClipIds.includes(clip.id))
    .map((clip) => clip.id);
  const selectedAudioClipCount = selectedAudioClipIds.length;
  const selectedMidiClipCount = selectedMidiClipIds.length;
  const setSelectedClipIds = (clipIds: string[]) =>
    props.setEditorSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
  return (
    <aside className={styles.panel} aria-label="Properties" data-properties-panel>
      <div className={styles.body}>
        {selectedTrack ? (
          <>
            <TrackInspector
              track={selectedTrack}
              session={props.session}
              audio={props.audio}
              onOpenDevices={props.onOpenDevices}
            />
            <TakeInspector
              session={props.session}
              selection={props.editorSelection}
              recordingActive={props.audio.recording.active || props.audio.recording.processing}
              recordingCommandPending={props.recordingCommandPending}
              onRecordAnotherTake={props.onRecordAnotherTake}
            />
          </>
        ) : selectedAudioClipCount + selectedMidiClipCount > 1 ? (
          <MultiClipInspector
            session={props.session}
            selectedAudioClipIds={selectedAudioClipIds}
            selectedMidiClipIds={selectedMidiClipIds}
            setSelectedClipIds={setSelectedClipIds}
          />
        ) : selectedMidiClipCount === 1 ? (
          <>
            <MidiClipInspector
              session={props.session}
              selectedClipIds={selectedMidiClipIds}
              setSelectedClipIds={setSelectedClipIds}
            />
            <TakeInspector
              session={props.session}
              selection={props.editorSelection}
              recordingActive={props.audio.recording.active || props.audio.recording.processing}
              recordingCommandPending={props.recordingCommandPending}
              onRecordAnotherTake={props.onRecordAnotherTake}
            />
          </>
        ) : selectedAudioClipCount === 1 ? (
          <>
            <AudioClipInspector
              session={props.session}
              selectedClipIds={selectedAudioClipIds}
              setSelectedClipIds={setSelectedClipIds}
              onSetLoopToClip={(clip) => {
                const timebase = props.session.arrangement.timebase;
                const endTicks = clipDurationTicks(clip, timebase);
                return api.updateTimelineLoopRange(
                  true,
                  tickToMusicalPosition(clip.startTick, timebase),
                  tickToMusicalPosition(clip.startTick + endTicks, timebase),
                );
              }}
            />
            <TakeInspector
              session={props.session}
              selection={props.editorSelection}
              recordingActive={props.audio.recording.active || props.audio.recording.processing}
              recordingCommandPending={props.recordingCommandPending}
              onRecordAnotherTake={props.onRecordAnotherTake}
            />
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
