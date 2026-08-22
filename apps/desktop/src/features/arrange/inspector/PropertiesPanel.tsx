import type { AudioStatus, CreativeSession, MissingDependency, PluginEntry } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { ArrangeClipInspector } from './ArrangeClipInspector';
import { MultiClipInspector } from './MultiClipInspector';
import { MidiClipInspector } from './MidiClipInspector';
import { TrackInspector } from './TrackInspector';
import { TakeInspector } from './TakeInspector';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { Icon } from '@/shared/ui/primitives';
import styles from './PropertiesPanel.module.css';

interface PropertiesPanelProps {
  audio: AudioStatus;
  recordingCommandPending: boolean;
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  arrangeSelection: ArrangeSelection;
  setArrangeSelection: (selection: ArrangeSelection) => void;
  missingDependencies: MissingDependency[];
  plugins: PluginEntry[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
  onRecordAnotherTake: (recordingSessionId: string) => void | Promise<void>;
  api: ArrangeInspectorApi;
}

export function PropertiesPanel(props: PropertiesPanelProps) {
  const selectedTrackId =
    props.arrangeSelection.kind === 'track' ? props.arrangeSelection.trackId : undefined;
  const selectedTrack = props.session.arrangement.tracks.find(
    (track) => track.id === selectedTrackId,
  );
  const selectedClipIds =
    props.arrangeSelection.kind === 'clips' ? props.arrangeSelection.clipIds : [];
  const selectedAudioClipIds = props.session.arrangement.audioClips
    .filter((clip) => selectedClipIds.includes(clip.id))
    .map((clip) => clip.id);
  const selectedMidiClipIds = props.session.arrangement.midiClips
    .filter((clip) => selectedClipIds.includes(clip.id))
    .map((clip) => clip.id);
  const selectedAudioClipCount = selectedAudioClipIds.length;
  const selectedMidiClipCount = selectedMidiClipIds.length;
  const setSelectedClipIds = (clipIds: string[]) =>
    props.setArrangeSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
  return (
    <aside className={styles.panel} aria-label="Properties" data-properties-panel>
      <div className={styles.body}>
        {selectedTrack ? (
          <>
            <TrackInspector
              track={selectedTrack}
              session={props.session}
              setSession={props.setSession}
              audio={props.audio}
              missingDeviceIds={props.missingDependencies
                .filter((item) => item.kind === 'plugin')
                .map((item) => item.id)}
              onDisableMissingPlugin={props.onDisableMissingPlugin}
              onReplaceMissingPlugin={props.onReplaceMissingPlugin}
              onRescanMissingPlugins={props.onRescanMissingPlugins}
              plugins={props.plugins}
              api={props.api}
            />
            <TakeInspector
              session={props.session}
              selection={props.arrangeSelection}
              setSession={props.setSession}
              recordingActive={props.audio.recording.active}
              recordingCommandPending={props.recordingCommandPending}
              onRecordAnotherTake={props.onRecordAnotherTake}
              api={props.api}
            />
          </>
        ) : selectedAudioClipCount + selectedMidiClipCount > 1 ? (
          <MultiClipInspector
            session={props.session}
            setSession={props.setSession}
            selectedAudioClipIds={selectedAudioClipIds}
            selectedMidiClipIds={selectedMidiClipIds}
            setSelectedClipIds={setSelectedClipIds}
            api={props.api}
          />
        ) : selectedMidiClipCount === 1 ? (
          <>
            <MidiClipInspector
              session={props.session}
              setSession={props.setSession}
              selectedClipIds={selectedMidiClipIds}
              setSelectedClipIds={setSelectedClipIds}
              api={props.api}
            />
            <TakeInspector
              session={props.session}
              selection={props.arrangeSelection}
              setSession={props.setSession}
              recordingActive={props.audio.recording.active}
              recordingCommandPending={props.recordingCommandPending}
              onRecordAnotherTake={props.onRecordAnotherTake}
              api={props.api}
            />
          </>
        ) : selectedAudioClipCount === 1 ? (
          <>
            <ArrangeClipInspector
              session={props.session}
              setSession={props.setSession}
              selectedClipIds={selectedAudioClipIds}
              setSelectedClipIds={setSelectedClipIds}
              api={props.api}
              onSetLoopToClip={(clip) => {
                const timebase = props.session.arrangement.timebase;
                const endTicks = Math.max(
                  1,
                  Math.round(
                    (clip.timelineDuration.frames / clip.timelineDuration.sampleRate) *
                      (timebase.bpm / 60) *
                      timebase.ppq,
                  ),
                );
                return props.api.updateTimelineLoopRange(
                  true,
                  clip.startTick,
                  clip.startTick + endTicks,
                );
              }}
            />
            <TakeInspector
              session={props.session}
              selection={props.arrangeSelection}
              setSession={props.setSession}
              recordingActive={props.audio.recording.active}
              recordingCommandPending={props.recordingCommandPending}
              onRecordAnotherTake={props.onRecordAnotherTake}
              api={props.api}
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
