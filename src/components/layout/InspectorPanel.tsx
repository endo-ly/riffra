import type { AudioStatus, BootstrapState, CreativeSession } from '@/lib/domain';
import { Icon } from '../shared/ui';
import styles from './InspectorPanel.module.css';

interface InspectorPanelProps {
  session: CreativeSession;
  audio: AudioStatus;
  boot: BootstrapState;
  focusMode: boolean;
  setFocusMode: (value: boolean) => void;
  selectedPluginName: string | null;
  selectedPluginVendor: string | null;
}

export function InspectorPanel(props: InspectorPanelProps) {
  const {
    session,
    audio,
    boot,
    focusMode,
    setFocusMode,
    selectedPluginName,
    selectedPluginVendor,
  } = props;
  return (
    <aside className="inspector-panel">
      <div className="panel-heading">
        <span>INSPECTOR</span>
        <button onClick={() => setFocusMode(true)}>×</button>
      </div>
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
          <strong>Signal</strong>
          <Icon name="chevron" />
        </header>
        <dl>
          <div>
            <dt>Input</dt>
            <dd>Mono</dd>
          </div>
          <div>
            <dt>Gain</dt>
            <dd>0.0 dB</dd>
          </div>
          <div>
            <dt>State</dt>
            <dd className="safe-label">Safe</dd>
          </div>
        </dl>
      </section>
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
            <dt>Bypassed blocks</dt>
            <dd>{audio.plugin?.bypassedBlocks ?? 0}</dd>
          </div>
        </dl>
      </section>
      <section>
        <header>
          <strong>Provenance</strong>
          <Icon name="chevron" />
        </header>
        <dl>
          <div>
            <dt>Session</dt>
            <dd>Scratch</dd>
          </div>
          <div>
            <dt>Updated</dt>
            <dd>
              {new Date(session.updatedAtMs).toLocaleTimeString('ja-JP', {
                hour: '2-digit',
                minute: '2-digit',
              })}
            </dd>
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
    </aside>
  );
}
