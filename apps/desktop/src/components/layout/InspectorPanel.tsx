import type {
  AudioStatus,
  BootstrapState,
  CreativeSession,
  MissingDependency,
  PluginEntry,
} from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeClipInspector } from '../arrange/ArrangeClipInspector';
import { MidiClipInspector } from '../arrange/MidiClipInspector';
import { TrackInspector } from '../arrange/TrackInspector';
import { TakeInspector } from '../arrange/TakeInspector';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';
import { Icon } from '../shared/ui';
import styles from './InspectorPanel.module.css';

interface InspectorPanelProps {
  audio: AudioStatus;
  boot: BootstrapState;
  focusMode: boolean;
  setFocusMode: (value: boolean) => void;
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  arrangeSelection: ArrangeSelection;
  setArrangeSelection: (selection: ArrangeSelection) => void;
  missingDependencies: MissingDependency[];
  plugins: PluginEntry[];
  onDisableMissingPlugin: (deviceId: string) => Promise<void>;
  onReplaceMissingPlugin: (deviceId: string, newPath: string) => Promise<void>;
  onRescanMissingPlugins: () => Promise<void>;
  api: NativeApi;
}

export function InspectorPanel(props: InspectorPanelProps) {
  const { boot, focusMode, setFocusMode } = props;
  const selectedTrackId =
    props.arrangeSelection.kind === 'track' ? props.arrangeSelection.trackId : undefined;
  const selectedTrack = props.session.arrangement.tracks.find(
    (track) => track.id === selectedTrackId,
  );
  return (
    <aside className="inspector-panel">
      <div className="panel-heading">
        <span>{props.session.workspace === 'arrange' ? 'CLIP INSPECTOR' : 'INSPECTOR'}</span>
      </div>
      {props.session.workspace === 'arrange' ? (
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
        ) : props.session.arrangement.midiClips.some(
            (clip) =>
              props.arrangeSelection.kind === 'clips' &&
              props.arrangeSelection.clipIds.includes(clip.id),
          ) ? (
          <MidiClipInspector
            session={props.session}
            setSession={props.setSession}
            selectedClipIds={
              props.arrangeSelection.kind === 'clips' ? props.arrangeSelection.clipIds : []
            }
            setSelectedClipIds={(clipIds) =>
              props.setArrangeSelection(
                clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' },
              )
            }
            api={props.api}
          />
        ) : (
          <>
            <ArrangeClipInspector
              session={props.session}
              setSession={props.setSession}
              selectedClipIds={
                props.arrangeSelection.kind === 'clips' ? props.arrangeSelection.clipIds : []
              }
              setSelectedClipIds={(clipIds) =>
                props.setArrangeSelection(
                  clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' },
                )
              }
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
        )
      ) : (
        <>
          <div className={styles.inspectorIdentity}>
            <span className={styles.inspectorArt}>
              {props.session.designContext.activeTool.slice(0, 2).toUpperCase()}
            </span>
            <div>
              <span className="eyebrow">DESIGN</span>
              <h3>{props.session.designContext.activeTool}</h3>
              <small>Always preserved</small>
            </div>
          </div>
          <section>
            <header>
              <strong>Data safety</strong>
              <Icon name="chevron" />
            </header>
            <p className="inspector-copy">
              世代付き自動保存が有効です。現在の作業はプロジェクトへ昇格しなくても保持されます。
            </p>
            <small className={styles.pathCopy}>{boot.dataRoot}</small>
          </section>
          <button className={styles.focusButton} onClick={() => setFocusMode(!focusMode)}>
            {focusMode ? 'Exit Focus Mode' : 'Focus Mode'}
          </button>
        </>
      )}
    </aside>
  );
}
