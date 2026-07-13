import type {
  AudioAnalysis,
  AudioStatus,
  BootstrapState,
  PluginEntry,
  RecordingAsset,
  Session,
  SeparationResult,
  Workspace,
} from '@/lib/domain';
import { Icon, Meter } from './ui';

export function WorkspaceHome({
  state,
  onWorkspace,
  onQuickRecord,
  recordingActive,
  onRecoverAudioDevice,
  onExportProject,
  onImportProject,
  onRestoreRecovery,
  onDismissRecovery,
  exportMessage,
}: {
  state: BootstrapState;
  onWorkspace: (workspace: Workspace) => void;
  onQuickRecord: () => void;
  recordingActive: boolean;
  onRecoverAudioDevice: () => void;
  onExportProject: () => void;
  onImportProject: () => void;
  onRestoreRecovery: (fileName: string) => void;
  onDismissRecovery: () => void;
  exportMessage: string;
}) {
  return (
    <>
      <section className="hero-card">
        <div>
          <span className="eyebrow">SCRATCH SESSION</span>
          <h1>音を出す準備ができています。</h1>
          <p>プロジェクトを作る必要はありません。演奏、音作り、録音は自動的に保全されます。</p>
        </div>
        <div className="hero-actions">
          <button className="primary" onClick={() => onWorkspace('play')}>
            <Icon name="play" />
            Playへ
          </button>
          <button className={`quiet ${recordingActive ? 'recording' : ''}`} onClick={onQuickRecord}>
            <span className="record-dot" />
            {recordingActive ? 'Stop Recording' : 'Quick Record'}
          </button>
          <button className="quiet" onClick={onExportProject}>
            Export Manifest
          </button>
          <button className="quiet" onClick={onImportProject}>
            Import Manifest
          </button>
        </div>
        <small className="export-message">{exportMessage}</small>
        {state.safeMode && (
          <div className="safe-mode-banner">
            <strong>SAFE MODE</strong>
            <span>
              External VST3, MIDI input, driver changes, live preview and new recordings are
              isolated. Project open, library access, offline analysis, render and export remain
              available. Restart without <code>--safe-mode</code> to reconnect devices.
            </span>
          </div>
        )}
        {state.recoveredFromGeneration && state.recoveryCandidates.length > 0 && (
          <div className="recovery-choice">
            <strong>RECOVERY CHOICE</strong>
            <span>
              The current session was recovered from an autosave generation. Choose a previous
              stable generation if needed.
            </span>
            <div>
              {state.recoveryCandidates.slice(0, 5).map((candidate) => (
                <button
                  className="text-button"
                  key={candidate.fileName}
                  onClick={() => onRestoreRecovery(candidate.fileName)}
                >
                  {candidate.projectName ?? 'Untitled'} ·{' '}
                  {new Date(candidate.updatedAtMs).toLocaleString('ja-JP')}
                </button>
              ))}
              <button className="text-button" onClick={onDismissRecovery}>
                Keep recovered session
              </button>
            </div>
          </div>
        )}
      </section>

      <section className="section-card audio-setup">
        <header>
          <div>
            <span className="eyebrow">AUDIO DEVICE</span>
            <h2>Sound First Setup</h2>
          </div>
          <span className="status-tag warning">ENGINE NEXT</span>
        </header>
        <div className="device-row">
          <div className="device-icon">
            <Icon name="bolt" />
          </div>
          <div>
            <strong>Native audio sidecar</strong>
            <small>
              {state.safeMode
                ? 'Safe Mode keeps external audio isolated'
                : 'WASAPI / ASIO connection is available through the safety chain'}
            </small>
          </div>
          <button className="text-button" disabled={state.safeMode} onClick={onRecoverAudioDevice}>
            {state.safeMode ? 'Safe Mode' : 'Recover device'}
          </button>
        </div>
        <div className="safety-row">
          <span>Startup volume</span>
          <strong>−18.0 dB</strong>
          <Meter value={34} />
          <span className="safe-label">SAFE</span>
        </div>
      </section>

      <section className="section-card continue-card">
        <header>
          <div>
            <span className="eyebrow">CONTINUE</span>
            <h2>前回の状態</h2>
          </div>
          <button className="icon-button">
            <Icon name="chevron" />
          </button>
        </header>
        <div className="continue-visual">
          <div className="wave-lines">
            {Array.from({ length: 48 }, (_, i) => (
              <i key={i} style={{ height: `${18 + ((i * 19) % 58)}%` }} />
            ))}
          </div>
          <div>
            <strong>{state.session.projectName ?? 'Untitled Scratch'}</strong>
            <small>{state.recoveredFromGeneration ? 'Recovery世代から復元' : '自動保存済み'}</small>
          </div>
        </div>
      </section>

      <section className="section-card recent-card">
        <header>
          <div>
            <span className="eyebrow">RECENT MEMORY</span>
            <h2>最近の制作資産</h2>
          </div>
          <button className="text-button">Open Library</button>
        </header>
        <div className="asset-strip">
          {['Glass Clean', 'Night Practice', 'Raw DI 07', 'Wide Space'].map((name, i) => (
            <button className="asset-tile" key={name}>
              <span className={`asset-art art-${i}`}>
                <i />
              </span>
              <strong>{name}</strong>
              <small>{i < 2 ? 'Rack' : i === 2 ? 'Recording' : 'Snapshot'}</small>
            </button>
          ))}
        </div>
      </section>
    </>
  );
}

export function WorkspacePlay({
  session,
  audio,
  plugins,
  missingPluginPaths,
  setSession,
  onTogglePluginBypass,
  onSetPluginParameter,
  onClearPlugin,
  onCaptureSnapshot,
  onRecallSnapshot,
}: {
  session: Session;
  audio: AudioStatus;
  plugins: PluginEntry[];
  missingPluginPaths: string[];
  setSession: (value: Session) => void;
  onTogglePluginBypass: (bypassed: boolean) => void;
  onSetPluginParameter: (index: number, value: number) => void;
  onClearPlugin: () => void;
  onCaptureSnapshot: (slot: 'A' | 'B') => void;
  onRecallSnapshot: (slot: 'A' | 'B') => void;
}) {
  const missingPaths = new Set(missingPluginPaths);
  const persistedPlugins = session.rack
    .filter((device) => device.kind === 'plugin')
    .map(
      (device) =>
        ({
          id: device.id,
          name: device.name,
          vendor: null,
          version: null,
          format: 'VST3',
          path: device.path ?? '',
          bundle: true,
          modifiedAtMs: null,
          scanState: device.path && missingPaths.has(device.path) ? 'quarantined' : 'validated',
        }) as PluginEntry,
    );
  const visiblePlugins = persistedPlugins.length ? persistedPlugins : plugins.slice(0, 3);
  const loadedBypassed = session.rack.find((device) => device.kind === 'plugin')?.bypassed ?? false;
  const hasSnapshotA = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:A');
  const hasSnapshotB = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:B');
  const setMacro = (macroId: string, value: number) => {
    const safeValue = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
    const macro = session.macros.find((item) => item.id === macroId);
    setSession({
      ...session,
      macros: session.macros.map((item) =>
        item.id === macroId ? { ...item, value: safeValue } : item,
      ),
    });
    if (macro?.parameterIndex != null) onSetPluginParameter(macro.parameterIndex, safeValue);
  };
  const mapMacro = (macroId: string, value: string) =>
    setSession({
      ...session,
      macros: session.macros.map((item) =>
        item.id === macroId
          ? { ...item, parameterIndex: value === '' ? null : Number(value) }
          : item,
      ),
    });
  return (
    <div className="workspace-scroll play-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">LIVE SIGNAL</span>
          <h1>Input → Tone → Output</h1>
        </div>
        <div className="snapshot-tabs">
          <button className={hasSnapshotA ? 'active' : ''} onClick={() => onRecallSnapshot('A')}>
            A
          </button>
          <button className={hasSnapshotB ? 'active' : ''} onClick={() => onRecallSnapshot('B')}>
            B
          </button>
          <button onClick={() => onCaptureSnapshot(hasSnapshotA ? 'B' : 'A')}>＋</button>
        </div>
      </section>
      <div className="signal-line" />
      <section className="rack-flow">
        <article className="rack-device input-device">
          <span className="device-order">IN</span>
          <div className="device-face">
            <Meter value={45} />
            <Meter value={40} />
          </div>
          <h3>Input 1</h3>
          <small>Mono · −12.4 dB</small>
        </article>
        {(visiblePlugins.length
          ? visiblePlugins
          : [{ id: 'placeholder', name: 'Add a VST3', vendor: null } as PluginEntry]
        ).map((plugin, index) => (
          <article
            className={`rack-device ${plugin.scanState === 'quarantined' ? 'missing-dependency' : ''}`}
            key={plugin.id}
          >
            <span className="device-order">{String(index + 1).padStart(2, '0')}</span>
            <div className={`device-face face-${index}`}>
              <span>{plugin.name.slice(0, 2).toUpperCase()}</span>
              <i />
            </div>
            <h3>{plugin.name}</h3>
            <small>
              {plugin.scanState === 'quarantined'
                ? 'Missing dependency'
                : (plugin.vendor ?? 'VST3 discovered')}
            </small>
            <div className="device-controls">
              <button onClick={() => onTogglePluginBypass(!loadedBypassed)}>
                {loadedBypassed ? 'Enable' : 'Bypass'}
              </button>
              <button onClick={onClearPlugin}>Remove</button>
              <strong>0.0 dB</strong>
            </div>
          </article>
        ))}
        <button className="add-device">
          <Icon name="plus" />
          <span>Add Device</span>
        </button>
        <article className="rack-device output-device">
          <span className="device-order">OUT</span>
          <div className="device-face">
            <Meter value={58} />
            <Meter value={51} />
          </div>
          <h3>Main Out</h3>
          <small>Safety limited</small>
        </article>
      </section>
      {audio.plugin?.loaded && audio.plugin.parameters.length > 0 && (
        <section className="section-card plugin-parameters">
          <header>
            <div>
              <span className="eyebrow">COMMON PARAMETER VIEW</span>
              <h2>{audio.plugin.parameters.length} VST3 parameters</h2>
            </div>
            <small>Native GUI is optional; changes stay inside the isolated rack.</small>
          </header>
          <div className="plugin-parameter-grid">
            {audio.plugin.parameters.slice(0, 48).map((parameter) => (
              <label className="plugin-parameter" key={parameter.index}>
                <span>
                  <strong>{parameter.name || `Parameter ${parameter.index + 1}`}</strong>
                  <small>
                    {Math.round(parameter.value * 100)}%
                    {parameter.automatable ? ' · automatable' : ''}
                  </small>
                </span>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.001"
                  value={parameter.value}
                  onChange={(event) =>
                    onSetPluginParameter(parameter.index, Number(event.target.value))
                  }
                />
              </label>
            ))}
          </div>
          {audio.plugin.parameters.length > 48 && (
            <small className="inspector-copy">
              Showing first 48 parameters; the rest remain available to the plugin.
            </small>
          )}
        </section>
      )}
      <section className="macro-section">
        <header>
          <div>
            <span className="eyebrow">MACROS</span>
            <h2>Performance controls</h2>
          </div>
          <small>
            {session.macros.filter((macro) => macro.parameterIndex != null).length} mapped
          </small>
        </header>
        <div className="macro-grid">
          {session.macros.map((macro) => (
            <label className="macro" key={macro.id}>
              <span
                className="knob"
                style={{ '--turn': `${-120 + macro.value * 240}deg` } as React.CSSProperties}
              >
                <i />
              </span>
              <strong>{macro.name}</strong>
              <input
                type="range"
                min="0"
                max="1"
                step="0.001"
                value={macro.value}
                onChange={(event) => setMacro(macro.id, Number(event.target.value))}
              />
              <small>{Math.round(macro.value * 100)}%</small>
              <select
                aria-label={`${macro.name} target`}
                value={macro.parameterIndex == null ? '' : macro.parameterIndex}
                onChange={(event) => mapMacro(macro.id, event.target.value)}
              >
                <option value="">Unmapped</option>
                {audio.plugin?.parameters.map((parameter) => (
                  <option value={parameter.index} key={parameter.index}>
                    {parameter.name || `Parameter ${parameter.index + 1}`}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
      </section>
      <label className="session-note">
        <span>Session note</span>
        <textarea
          value={session.note}
          onChange={(event) => setSession({ ...session, note: event.target.value })}
          placeholder="意図、比較対象、使用場面を記録…"
        />
      </label>
    </div>
  );
}

export function EmptyWorkspace({ workspace }: { workspace: Workspace }) {
  const copy: Record<Workspace, { title: string; body: string; action: string }> = {
    home: { title: '', body: '', action: '' },
    play: { title: '', body: '', action: '' },
    arrange: {
      title: 'Timelineへ素材を置く',
      body: 'Recording、Audio、MIDIを非破壊で配置し、同期位置を保ったまま編集します。',
      action: 'Inboxを開く',
    },
    sample: {
      title: '音から楽器を作る',
      body: 'Audioを切り出し、PadまたはKeyboardへ割り当て、再利用可能なInstrumentとして保存します。',
      action: 'Audioを選択',
    },
    analyze: {
      title: '測定して、理解する',
      body: 'Waveform、Loudness、Spectrum、PhaseをReferenceと音量を揃えて比較します。',
      action: '素材をAnalyze',
    },
    separate: {
      title: 'StemをBackgroundで分離',
      body: 'Originalと結果を同期試聴し、Artifactの可能性を確認してからTimelineやLibraryへ送ります。',
      action: 'Jobを作成',
    },
  };
  const item = copy[workspace];
  return (
    <div className="empty-workspace">
      <span className={`empty-orbit orbit-${workspace}`}>
        <i />
        <b />
      </span>
      <span className="eyebrow">{workspace.toUpperCase()} WORKSPACE</span>
      <h1>{item.title}</h1>
      <p>{item.body}</p>
      <button className="primary">
        <Icon name="plus" />
        {item.action}
      </button>
      <small>
        このワークスペースの処理エンジンは後続ゲートで接続されます。Scratch Sessionは維持されます。
      </small>
    </div>
  );
}

export function WorkspaceAnalyze({ analysis }: { analysis: AudioAnalysis | null }) {
  if (!analysis) {
    return (
      <div className="empty-workspace">
        <span className="empty-orbit orbit-analyze">
          <i />
          <b />
        </span>
        <span className="eyebrow">ANALYZE WORKSPACE</span>
        <h1>測定して、理解する</h1>
        <p>
          LibraryのRecordingsからProcessed WAVを選ぶと、音量・位相・簡易スペクトルを確認できます。
        </p>
        <small>解析はオフラインで実行され、元の録音ファイルは変更されません。</small>
      </div>
    );
  }
  return (
    <div className="workspace-scroll analysis-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">ANALYSIS RESULT</span>
          <h1>{analysis.path.split('\\').pop() ?? 'Audio'}</h1>
        </div>
        <span className="status-tag">READ ONLY</span>
      </section>
      <section className="section-card waveform-card">
        <span className="eyebrow">WAVEFORM</span>
        <div className="waveform-analysis">
          {analysis.waveform.map((value, index) => (
            <i key={index} style={{ height: `${Math.max(4, value * 100)}%` }} />
          ))}
        </div>
      </section>
      <section className="analysis-grid">
        <article className="section-card">
          <span className="eyebrow">LEVEL</span>
          <h2>{analysis.rmsDb.toFixed(1)} dB RMS</h2>
          <p>
            Peak {analysis.peakDb.toFixed(1)} dBFS · True peak {analysis.truePeakDb.toFixed(1)} dBFS
          </p>
        </article>
        <article className="section-card">
          <span className="eyebrow">DYNAMICS</span>
          <h2>{analysis.dynamicRangeDb.toFixed(1)} dB</h2>
          <p>{analysis.clippingSamples.toLocaleString()} clipped samples · estimated from PCM</p>
        </article>
        <article className="section-card">
          <span className="eyebrow">SPECTRUM</span>
          <h2>{analysis.spectrumPeakHz ? `${analysis.spectrumPeakHz.toFixed(1)} Hz` : '—'}</h2>
          <p>簡易スペクトルピーク</p>
        </article>
        <article className="section-card">
          <span className="eyebrow">PHASE</span>
          <h2>
            {analysis.phaseCorrelation == null ? 'Mono' : analysis.phaseCorrelation.toFixed(3)}
          </h2>
          <p>
            {analysis.phaseCorrelation == null ? 'ステレオ相関なし' : 'Left / Right correlation'}
          </p>
        </article>
        <article className="section-card">
          <span className="eyebrow">TIMING</span>
          <h2>{(analysis.durationMs / 1000).toFixed(2)} s</h2>
          <p>
            {analysis.sampleRate} Hz · {analysis.channels} ch · {analysis.bitsPerSample} bit
          </p>
        </article>
      </section>
    </div>
  );
}

export function WorkspaceArrange({
  session,
  setSession,
  recordings,
  onPlaceRecording,
}: {
  session: Session;
  setSession: (value: Session) => void;
  recordings: RecordingAsset[];
  onPlaceRecording: (recording: RecordingAsset) => void;
}) {
  const timelineEnd = Math.max(
    10_000,
    ...session.timeline.map((clip) => clip.startMs + clip.durationMs),
  );
  const toggleTrack = (id: string, field: 'muted' | 'solo') =>
    setSession({
      ...session,
      tracks: session.tracks.map((track) =>
        track.id === id ? { ...track, [field]: !track[field] } : track,
      ),
    });
  const updateTrack = (id: string, field: 'gainDb' | 'pan', value: number) =>
    setSession({
      ...session,
      tracks: session.tracks.map((track) =>
        track.id !== id
          ? track
          : {
              ...track,
              [field]:
                field === 'gainDb'
                  ? Math.max(-90, Math.min(24, value))
                  : Math.max(-1, Math.min(1, value)),
            },
      ),
    });
  const addTrack = () => {
    const name = window.prompt('Track name', `Track ${session.tracks.length + 1}`)?.trim();
    if (!name) return;
    setSession({
      ...session,
      tracks: [
        ...session.tracks,
        {
          id: `track:${Date.now()}`,
          name: name.slice(0, 80),
          gainDb: 0,
          pan: 0,
          muted: false,
          solo: false,
        },
      ],
    });
  };
  return (
    <div className="workspace-scroll arrange-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">NON-DESTRUCTIVE TIMELINE</span>
          <h1>Arrange ideas without moving sources</h1>
        </div>
        <span className="status-tag">
          {session.timeline.length} CLIPS · {session.tracks.length} TRACKS
        </span>
      </section>
      <section className="section-card track-mixer">
        <header>
          <div>
            <span className="eyebrow">TRACK MIXER</span>
            <h2>Shared lanes and safe mix state</h2>
          </div>
          <button className="text-button" onClick={addTrack}>
            Add track
          </button>
        </header>
        <div className="track-mixer-grid">
          {session.tracks.map((track) => (
            <div className={`track-mixer-row ${track.muted ? 'muted' : ''}`} key={track.id}>
              <strong>{track.name}</strong>
              <label>
                <span>Gain</span>
                <input
                  type="number"
                  min="-90"
                  max="24"
                  step="0.5"
                  value={track.gainDb}
                  onChange={(event) => updateTrack(track.id, 'gainDb', Number(event.target.value))}
                />
              </label>
              <label>
                <span>Pan</span>
                <input
                  type="number"
                  min="-1"
                  max="1"
                  step="0.05"
                  value={track.pan}
                  onChange={(event) => updateTrack(track.id, 'pan', Number(event.target.value))}
                />
              </label>
              <button className="text-button" onClick={() => toggleTrack(track.id, 'muted')}>
                {track.muted ? 'Unmute' : 'Mute'}
              </button>
              <button className="text-button" onClick={() => toggleTrack(track.id, 'solo')}>
                {track.solo ? 'Unsolo' : 'Solo'}
              </button>
            </div>
          ))}
        </div>
      </section>
      <section className="section-card timeline-card">
        <div className="timeline-ruler">
          <span>00:00</span>
          <span>{(timelineEnd / 1000).toFixed(1)} s</span>
        </div>
        <div className="timeline-lane">
          {session.timeline.length === 0 && (
            <small>InboxのRecordingを右側からTimelineへ配置できます。</small>
          )}
          {session.timeline.map((clip) => {
            const track = session.tracks.find((item) => item.id === clip.trackId);
            return (
              <article
                className={`timeline-clip ${clip.muted || track?.muted ? 'muted' : ''}`}
                key={clip.id}
                style={{
                  left: `${(clip.startMs / timelineEnd) * 100}%`,
                  width: `${Math.max(8, (clip.durationMs / timelineEnd) * 100)}%`,
                }}
              >
                <strong>{clip.name}</strong>
                <small>
                  {track?.name ?? 'Main'} · {clip.gainDb.toFixed(1)} dB ·{' '}
                  {clip.muted ? 'Muted' : 'Source linked'}
                </small>
              </article>
            );
          })}
        </div>
      </section>
      <section className="section-card arrange-sources">
        <header>
          <div>
            <span className="eyebrow">INBOX SOURCES</span>
            <h2>素材を配置</h2>
          </div>
          <small>元ファイルは変更されません</small>
        </header>
        {recordings.length === 0 ? (
          <p className="inspector-copy">まだ録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className="source-row" key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button className="text-button" onClick={() => onPlaceRecording(recording)}>
                Place
              </button>
            </div>
          ))
        )}
      </section>
    </div>
  );
}

export function WorkspaceSample({
  session,
  recordings,
  onCreateSamplePad,
  onPreviewPad,
}: {
  session: Session;
  recordings: RecordingAsset[];
  onCreateSamplePad: (recording: RecordingAsset) => void;
  onPreviewPad: (pad: Session['samplePads'][number]) => void;
}) {
  const pads = Array.from({ length: 16 }, (_, index) => session.samplePads[index] ?? null);
  return (
    <div className="workspace-scroll sample-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">SAMPLE INSTRUMENT</span>
          <h1>Audio → Pad / Keyboard</h1>
        </div>
        <span className="status-tag">SOURCE MAPPING</span>
      </section>
      <section className="section-card pad-card">
        <header>
          <div>
            <span className="eyebrow">PADS</span>
            <h2>{session.samplePads.length} mapped</h2>
          </div>
          <small>Playback engine follows this mapping gate</small>
        </header>
        <div className="pad-grid">
          {pads.map((pad, index) => (
            <button
              className={`sample-pad ${pad ? 'filled' : 'empty'}`}
              key={pad?.id ?? `empty-${index}`}
              onClick={pad ? () => onPreviewPad(pad) : undefined}
              aria-label={pad ? `Preview ${pad.name}` : `Empty pad ${index + 1}`}
            >
              <strong>{pad?.name ?? `Pad ${index + 1}`}</strong>
              <small>{pad ? `MIDI ${pad.midiKey}` : 'Empty'}</small>
            </button>
          ))}
        </div>
      </section>
      <section className="section-card sample-sources">
        <header>
          <div>
            <span className="eyebrow">SOURCES</span>
            <h2>録音をPadへ割り当てる</h2>
          </div>
          <small>元ファイルは変更されません</small>
        </header>
        {recordings.length === 0 ? (
          <p className="inspector-copy">Inboxに録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className="source-row" key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button className="text-button" onClick={() => onCreateSamplePad(recording)}>
                Map to Pad
              </button>
            </div>
          ))
        )}
      </section>
    </div>
  );
}

export function WorkspaceSeparate({
  recordings,
  results,
  busyId,
  message,
  previewingPath,
  onSeparate,
  onPreview,
  onStop,
  onAddToTimeline,
}: {
  recordings: RecordingAsset[];
  results: SeparationResult[];
  busyId: string | null;
  message: string;
  previewingPath: string | null;
  onSeparate: (recording: RecordingAsset) => void;
  onPreview: (path: string) => void;
  onStop: () => void;
  onAddToTimeline: (path: string, name: string) => void;
}) {
  return (
    <div className="workspace-scroll separate-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">SEPARATE WORKSPACE</span>
          <h1>Preserve the source, derive channel assets</h1>
        </div>
        <span className="status-tag">CHANNEL SPLIT FALLBACK</span>
      </section>
      <section className="section-card separate-card">
        <header>
          <div>
            <span className="eyebrow">OFFLINE JOB</span>
            <h2>Stereo channel split</h2>
          </div>
          <small>Creates immutable Left / Right WAV assets</small>
        </header>
        <p className="inspector-copy">
          This local fallback separates stereo channels without claiming vocal or instrument stems.
          The original WAV is never overwritten.
        </p>
        {recordings.length === 0 ? (
          <p className="inspector-copy">Inboxに録音がありません。</p>
        ) : (
          recordings.slice(0, 12).map((recording) => (
            <div className="source-row" key={recording.id}>
              <div>
                <strong>{recording.name}</strong>
                <small>
                  {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                </small>
              </div>
              <button
                className="text-button"
                disabled={busyId === recording.id}
                onClick={() => onSeparate(recording)}
              >
                {busyId === recording.id ? 'Running…' : 'Split stereo'}
              </button>
            </div>
          ))
        )}
        <small className="separate-message">{message}</small>
      </section>
      <section className="section-card separate-results">
        <header>
          <div>
            <span className="eyebrow">DERIVED ASSETS</span>
            <h2>{results.length} completed jobs</h2>
          </div>
          <small>Manifest-backed provenance</small>
        </header>
        {results.length === 0 ? (
          <p className="inspector-copy">No separation result has been created yet.</p>
        ) : (
          results.slice(0, 8).map((result) => {
            const sourceName = result.sourcePath.split('\\').pop() ?? 'Stem';
            return (
              <article className="separation-result" key={result.id}>
                <div>
                  <strong>{sourceName}</strong>
                  <small>
                    {new Date(result.createdAtMs).toLocaleString('ja-JP')} · {result.state}
                  </small>
                </div>
                <div className="separation-paths">
                  <span>
                    LEFT <code>{result.leftPath}</code>
                    <button
                      className="text-button"
                      onClick={() =>
                        previewingPath === result.leftPath ? onStop() : onPreview(result.leftPath)
                      }
                    >
                      {previewingPath === result.leftPath ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className="text-button"
                      onClick={() => onAddToTimeline(result.leftPath, `Left · ${sourceName}`)}
                    >
                      Add to Timeline
                    </button>
                  </span>
                  <span>
                    RIGHT <code>{result.rightPath}</code>
                    <button
                      className="text-button"
                      onClick={() =>
                        previewingPath === result.rightPath ? onStop() : onPreview(result.rightPath)
                      }
                    >
                      {previewingPath === result.rightPath ? 'Stop' : 'Preview'}
                    </button>
                    <button
                      className="text-button"
                      onClick={() => onAddToTimeline(result.rightPath, `Right · ${sourceName}`)}
                    >
                      Add to Timeline
                    </button>
                  </span>
                </div>
              </article>
            );
          })
        )}
      </section>
    </div>
  );
}
