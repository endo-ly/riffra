import type {
  AudioStatus,
  BootstrapState,
  CreativeSession,
  DesktopViewState,
  MissingDependency,
  PluginEntry,
} from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { ArrangeClipInspector } from './ArrangeClipInspector';
import { MultiClipInspector } from './MultiClipInspector';
import { MidiClipInspector } from './MidiClipInspector';
import { TrackInspector } from './TrackInspector';
import { TakeInspector } from './TakeInspector';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { Icon } from '@/shared/ui/primitives';
import surface from '@/shared/ui/Surface.module.css';
import styles from './InspectorPanel.module.css';

interface InspectorPanelProps {
  audio: AudioStatus;
  boot: BootstrapState;
  focusMode: boolean;
  setFocusMode: (value: boolean) => void;
  session: CreativeSession;
  viewState: DesktopViewState;
  setSession: (session: CreativeSession) => void;
  arrangeSelection: ArrangeSelection;
  setArrangeSelection: (selection: ArrangeSelection) => void;
  missingDependencies: MissingDependency[];
  plugins: PluginEntry[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
  api: ArrangeInspectorApi;
}

export function InspectorPanel(props: InspectorPanelProps) {
  const { boot, focusMode, setFocusMode } = props;
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
  const title = getInspectorTitle(
    props.viewState.workspace,
    Boolean(selectedTrack),
    selectedAudioClipCount,
    selectedMidiClipCount,
  );
  return (
    <aside className={styles.panel} data-inspector-panel>
      <div className={styles.heading}>
        <span>{title}</span>
      </div>
      <div className={styles.body}>
        {props.viewState.workspace === 'arrange' ? (
          selectedTrack ? (
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
            <MidiClipInspector
              session={props.session}
              setSession={props.setSession}
              selectedClipIds={selectedMidiClipIds}
              setSelectedClipIds={setSelectedClipIds}
              api={props.api}
            />
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
                api={props.api}
              />
            </>
          ) : null
        ) : (
          <>
            <div className={styles.designIdentity}>
              <span className={styles.designArt}>
                {props.viewState.designContext.activeTool.slice(0, 2).toUpperCase()}
              </span>
              <div>
                <span className={surface.eyebrow}>DESIGN</span>
                <h3>{props.viewState.designContext.activeTool}</h3>
                <small>Always preserved</small>
              </div>
            </div>
            <section className={styles.designSection}>
              <header>
                <strong>Data safety</strong>
                <Icon name="chevron" />
              </header>
              <p className={styles.designCopy}>
                世代付き自動保存が有効です。現在の作業はプロジェクトへ昇格しなくても保持されます。
              </p>
              <small className={styles.pathCopy}>{boot.dataRoot}</small>
            </section>
            <button className={styles.focusButton} onClick={() => setFocusMode(!focusMode)}>
              {focusMode ? 'Exit Focus Mode' : 'Focus Mode'}
            </button>
          </>
        )}
      </div>
    </aside>
  );
}

function getInspectorTitle(
  workspace: DesktopViewState['workspace'],
  hasSelectedTrack: boolean,
  audioClipCount: number,
  midiClipCount: number,
) {
  if (workspace !== 'arrange') return 'INSPECTOR';
  if (hasSelectedTrack) return 'TRACK';
  if (audioClipCount > 0 && midiClipCount === 0)
    return audioClipCount === 1 ? 'AUDIO CLIP' : 'AUDIO CLIPS';
  if (midiClipCount > 0 && audioClipCount === 0)
    return midiClipCount === 1 ? 'MIDI CLIP' : 'MIDI CLIPS';
  if (audioClipCount > 0 || midiClipCount > 0) return 'CLIPS';
  return 'INSPECTOR';
}
