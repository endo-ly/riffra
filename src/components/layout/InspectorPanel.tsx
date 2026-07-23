import type { AudioStatus, BootstrapState, CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ArrangeClipInspector } from '../arrange/ArrangeClipInspector';
import { MidiClipInspector } from '../arrange/MidiClipInspector';
import { Icon } from '../shared/ui';
import styles from './InspectorPanel.module.css';

interface InspectorPanelProps {
  audio: AudioStatus;
  boot: BootstrapState;
  focusMode: boolean;
  setFocusMode: (value: boolean) => void;
  selectedPluginName: string | null;
  selectedPluginVendor: string | null;
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  arrangeSelection: string[];
  setArrangeSelection: (ids: string[]) => void;
  api: NativeApi;
}

export function InspectorPanel(props: InspectorPanelProps) {
  const { audio, boot, focusMode, setFocusMode, selectedPluginName, selectedPluginVendor } = props;
  return (
    <aside className="inspector-panel">
      <div className="panel-heading">
        <span>{props.session.workspace === 'arrange' ? 'CLIP INSPECTOR' : 'INSPECTOR'}</span>
      </div>
      {props.session.workspace === 'arrange' ? (
        props.session.arrangement.midiClips.some((clip) =>
          props.arrangeSelection.includes(clip.id),
        ) ? (
          <MidiClipInspector
            session={props.session}
            setSession={props.setSession}
            selectedClipIds={props.arrangeSelection}
            setSelectedClipIds={props.setArrangeSelection}
            api={props.api}
          />
        ) : (
          <ArrangeClipInspector
            session={props.session}
            setSession={props.setSession}
            selectedClipIds={props.arrangeSelection}
            setSelectedClipIds={props.setArrangeSelection}
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
              void props.api
                .updateTimelineLoopRange(true, clip.startTick, clip.startTick + endTicks)
                .then(props.setSession);
            }}
          />
        )
      ) : (
        <>
          <div className={styles.inspectorIdentity}>
            <span className={styles.inspectorArt}>
              {selectedPluginName?.slice(0, 2).toUpperCase() ?? 'SS'}
            </span>
            <div>
              <span className="eyebrow">{selectedPluginName ? 'PLUGIN' : 'SESSION'}</span>
              <h3>{selectedPluginName ?? 'Scratch Session'}</h3>
              <small>{selectedPluginVendor ?? 'Always preserved'}</small>
            </div>
          </div>
          <section>
            <header>
              <strong>Tone engine</strong>
              <Icon name="chevron" />
            </header>
            <dl>
              <div>
                <dt>Rack</dt>
                <dd className={audio.plugin?.loaded ? 'safe-label' : ''}>
                  {audio.plugin?.loaded ? 'Loaded' : 'Empty'}
                </dd>
              </div>
              <div>
                <dt>VST3</dt>
                <dd>{audio.plugin?.name ?? '—'}</dd>
              </div>
              <div>
                <dt>State</dt>
                <dd>{audio.plugin?.bypassed ? 'Bypassed' : 'Active'}</dd>
              </div>
              <div>
                <dt>Layout</dt>
                <dd>
                  {audio.plugin?.loaded
                    ? `${audio.plugin.inputChannels} in / ${audio.plugin.outputChannels} out`
                    : '—'}
                </dd>
              </div>
              <div>
                <dt>Bypassed blocks</dt>
                <dd>{audio.plugin?.bypassedBlocks ?? 0}</dd>
              </div>
              <div>
                <dt>Processed blocks</dt>
                <dd>{audio.plugin?.processedBlocks ?? 0}</dd>
              </div>
              <div>
                <dt>Contention blocks</dt>
                <dd>{audio.plugin?.contentionBlocks ?? 0}</dd>
              </div>
              <div>
                <dt>Transition blocks</dt>
                <dd>{audio.plugin?.transitionBlocks ?? 0}</dd>
              </div>
            </dl>
          </section>
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
