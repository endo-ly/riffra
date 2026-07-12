import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AiChangeSet, AudioAnalysis, AudioDeviceProbe, AudioStatus, BootstrapState, LibraryAsset, MidiClip, MidiEvent, MidiNote, MidiProbe, PluginEntry, RecordingAsset, RenderOptions, RenderResult, ScratchSession, SeparationResult, Workspace } from "./domain";
import { compareAnalyses } from "./domain";
import { analyzeAudio, bootstrap, clearPlugin, closeMidiInput, configureSamplePads, exportMidi, exportScratchSession, getAudioStatus, importScratchSession, listRecordings, listSeparations, loadPlugin, openMidiInput, previewSample, probeAudioDevices, probeMidiDevices, readMidiEvents, recoverAudioDevice, relatedLibraryAssets, renderTimeline, renderTimelineStems, restoreRecoveryGeneration, saveScratch, scanVst3Folder, searchLibrary, separateChannels, setAudioDriver, setEmergencyMute, setMasterGainDb, setPluginBypassed, setPluginParameter, setPluginState, startRecording, stopRecording, stopSamplePreview, updateLibraryAsset } from "./native";

const workspaces: Array<{ id: Workspace; label: string; key: string }> = [
  { id: "home", label: "Home", key: "1" },
  { id: "play", label: "Play", key: "2" },
  { id: "arrange", label: "Arrange", key: "3" },
  { id: "sample", label: "Sample", key: "4" },
  { id: "analyze", label: "Analyze", key: "5" },
  { id: "separate", label: "Separate", key: "6" },
];

const librarySections = ["Plugins", "Racks", "Presets", "Samples", "Recordings", "MIDI", "Projects", "References"];

function Icon({ name }: { name: string }) {
  const paths: Record<string, string> = {
    search: "M11 4a7 7 0 1 0 4.9 12l4.55 4.55 1.4-1.4-4.55-4.55A7 7 0 0 0 11 4Zm0 2a5 5 0 1 1 0 10 5 5 0 0 1 0-10Z",
    play: "M8 5v14l11-7Z",
    stop: "M7 7h10v10H7Z",
    record: "M12 5a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z",
    loop: "M7 7h10V4l4 4-4 4V9H7a3 3 0 0 0-3 3v1H2v-1a5 5 0 0 1 5-5Zm10 10H7v3l-4-4 4-4v3h10a3 3 0 0 0 3-3v-1h2v1a5 5 0 0 1-5 5Z",
    plus: "M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6Z",
    chevron: "m9 18 6-6-6-6",
    bolt: "m13 2-9 12h7l-1 8 9-12h-7Z",
    command: "M9 6a3 3 0 1 0-3 3h3V6Zm2 0v3h2V6h-2Zm4 0v3h3a3 3 0 1 0-3-3ZM9 11H6a3 3 0 1 0 3 3v-3Zm2 0v2h2v-2h-2Zm4 0v3a3 3 0 1 0 3-3h-3Zm-6 5H6a1 1 0 1 1 1-1h2v1Zm2-1h2v2h-2v-2Zm4 0h2a1 1 0 1 1-2 1v-1Z",
  };
  return <svg aria-hidden="true" viewBox="0 0 24 24"><path d={paths[name] ?? paths.plus} /></svg>;
}

function Meter({ value, danger = false }: { value: number; danger?: boolean }) {
  return <span className={`meter ${danger ? "meter-danger" : ""}`}><i style={{ width: `${Math.max(2, Math.min(100, value))}%` }} /></span>;
}

function WorkspaceHome({ state, onWorkspace, onQuickRecord, recordingActive, onRecoverAudioDevice, onExportProject, onImportProject, onRestoreRecovery, onDismissRecovery, exportMessage }: { state: BootstrapState; onWorkspace: (workspace: Workspace) => void; onQuickRecord: () => void; recordingActive: boolean; onRecoverAudioDevice: () => void; onExportProject: () => void; onImportProject: () => void; onRestoreRecovery: (fileName: string) => void; onDismissRecovery: () => void; exportMessage: string }) {
  return (
    <div className="workspace-scroll home-grid">
      <section className="hero-card">
        <div>
          <span className="eyebrow">SCRATCH SESSION</span>
          <h1>音を出す準備ができています。</h1>
          <p>プロジェクトを作る必要はありません。演奏、音作り、録音は自動的に保全されます。</p>
        </div>
        <div className="hero-actions">
          <button className="primary" onClick={() => onWorkspace("play")}><Icon name="play" />Playへ</button>
          <button className={`quiet ${recordingActive ? "recording" : ""}`} onClick={onQuickRecord}><span className="record-dot" />{recordingActive ? "Stop Recording" : "Quick Record"}</button>
          <button className="quiet" onClick={onExportProject}>Export Manifest</button>
          <button className="quiet" onClick={onImportProject}>Import Manifest</button>
        </div>
        <small className="export-message">{exportMessage}</small>
        {state.safeMode && <div className="safe-mode-banner"><strong>SAFE MODE</strong><span>External VST3, MIDI input, driver changes, live preview and new recordings are isolated. Project open, library access, offline analysis, render and export remain available. Restart without <code>--safe-mode</code> to reconnect devices.</span></div>}
        {state.recoveredFromGeneration && state.recoveryCandidates.length > 0 && <div className="recovery-choice"><strong>RECOVERY CHOICE</strong><span>The current session was recovered from an autosave generation. Choose a previous stable generation if needed.</span><div>{state.recoveryCandidates.slice(0, 5).map((candidate) => <button className="text-button" key={candidate.fileName} onClick={() => onRestoreRecovery(candidate.fileName)}>{candidate.projectName ?? "Untitled"} · {new Date(candidate.updatedAtMs).toLocaleString("ja-JP")}</button>)}<button className="text-button" onClick={onDismissRecovery}>Keep recovered session</button></div></div>}
      </section>

      <section className="section-card audio-setup">
        <header><div><span className="eyebrow">AUDIO DEVICE</span><h2>Sound First Setup</h2></div><span className="status-tag warning">ENGINE NEXT</span></header>
        <div className="device-row"><div className="device-icon"><Icon name="bolt" /></div><div><strong>Native audio sidecar</strong><small>{state.safeMode ? "Safe Mode keeps external audio isolated" : "WASAPI / ASIO connection is available through the safety chain"}</small></div><button className="text-button" disabled={state.safeMode} onClick={onRecoverAudioDevice}>{state.safeMode ? "Safe Mode" : "Recover device"}</button></div>
        <div className="safety-row"><span>Startup volume</span><strong>−18.0 dB</strong><Meter value={34} /><span className="safe-label">SAFE</span></div>
      </section>

      <section className="section-card continue-card">
        <header><div><span className="eyebrow">CONTINUE</span><h2>前回の状態</h2></div><button className="icon-button"><Icon name="chevron" /></button></header>
        <div className="continue-visual">
          <div className="wave-lines">{Array.from({ length: 48 }, (_, i) => <i key={i} style={{ height: `${18 + ((i * 19) % 58)}%` }} />)}</div>
          <div><strong>{state.session.projectName ?? "Untitled Scratch"}</strong><small>{state.recoveredFromGeneration ? "Recovery世代から復元" : "自動保存済み"}</small></div>
        </div>
      </section>

      <section className="section-card recent-card">
        <header><div><span className="eyebrow">RECENT MEMORY</span><h2>最近の制作資産</h2></div><button className="text-button">Open Library</button></header>
        <div className="asset-strip">
          {["Glass Clean", "Night Practice", "Raw DI 07", "Wide Space"].map((name, i) => (
            <button className="asset-tile" key={name}><span className={`asset-art art-${i}`}><i /></span><strong>{name}</strong><small>{i < 2 ? "Rack" : i === 2 ? "Recording" : "Snapshot"}</small></button>
          ))}
        </div>
      </section>
    </div>
  );
}

function AudioDevices({ probe, onRefresh }: { probe: AudioDeviceProbe; onRefresh: () => void }) {
  return <section className="section-card audio-device-list"><header><div><span className="eyebrow">WINDOWS DEVICES</span><h2>WASAPI / ASIO / MIDI</h2></div><button className="text-button" onClick={onRefresh}>Refresh</button></header>{probe.drivers.length === 0 ? <p className="inspector-copy">No audio driver list is available yet.</p> : probe.drivers.map((driver) => <div className="audio-driver-row" key={driver.name}><strong>{driver.name}</strong><small>{driver.inputs.length} inputs · {driver.outputs.length} outputs</small></div>)}<small className="device-probe-message">{probe.message} · MIDI {probe.midiInputs.length} in / {probe.midiOutputs.length} out</small></section>;
}

function AudioDriverPicker({ probe, current, sampleRate, bufferSize, onSelect }: { probe: AudioDeviceProbe; current: string | null; sampleRate: number | null; bufferSize: number | null; onSelect: (driver: string, sampleRate: number, bufferSize: number) => void }) {
  if (!probe.drivers.length) return null;
  const activeDriver = current ?? probe.drivers[0].name;
  const selectedRate = sampleRate ?? 48_000;
  const selectedBuffer = bufferSize ?? 256;
  return <section className="section-card audio-driver-picker"><header><div><span className="eyebrow">DRIVER ROUTING</span><h2>Choose a safe audio backend</h2></div><small>Switching re-enters emergency mute</small></header><div className="audio-format-picker"><label><span>Sample rate</span><select value={selectedRate} onChange={(event) => onSelect(activeDriver, Number(event.target.value), selectedBuffer)}>{[44_100, 48_000, 88_200, 96_000].map((rate) => <option value={rate} key={rate}>{rate.toLocaleString()} Hz</option>)}</select></label><label><span>Buffer</span><select value={selectedBuffer} onChange={(event) => onSelect(activeDriver, selectedRate, Number(event.target.value))}>{[64, 128, 256, 512, 1024].map((buffer) => <option value={buffer} key={buffer}>{buffer} samples</option>)}</select></label></div><div className="driver-picker-grid">{probe.drivers.map((driver) => <button className={`driver-choice ${driver.name === current ? "active" : ""}`} key={driver.name} onClick={() => onSelect(driver.name, selectedRate, selectedBuffer)}><strong>{driver.name}</strong><small>{driver.name === current ? "Current" : "Use this driver"}</small></button>)}</div></section>;
}

function CaptureSettings({ session, setSession }: { session: ScratchSession; setSession: (value: ScratchSession) => void }) {
  return <section className="section-card capture-settings"><header><div><span className="eyebrow">CAPTURE</span><h2>Quick Record timing</h2></div><small>Stored with this Scratch Session</small></header><label><span>Visual count-in</span><select value={session.countInBeats} onChange={(event) => setSession({ ...session, countInBeats: Number(event.target.value) })}>{[0, 1, 2, 3, 4, 8].map((beats) => <option value={beats} key={beats}>{beats === 0 ? "Off" : `${beats} beats`}</option>)}</select></label><p className="inspector-copy">When enabled, recording starts after the countdown. Existing audio is never captured during the count-in.</p></section>;
}

function WorkspacePlay({ session, audio, plugins, missingPluginPaths, setSession, onTogglePluginBypass, onSetPluginParameter, onClearPlugin, onCaptureSnapshot, onRecallSnapshot }: { session: ScratchSession; audio: AudioStatus; plugins: PluginEntry[]; missingPluginPaths: string[]; setSession: (value: ScratchSession) => void; onTogglePluginBypass: (bypassed: boolean) => void; onSetPluginParameter: (index: number, value: number) => void; onClearPlugin: () => void; onCaptureSnapshot: (slot: "A" | "B") => void; onRecallSnapshot: (slot: "A" | "B") => void }) {
  const missingPaths = new Set(missingPluginPaths);
  const persistedPlugins = session.rack
    .filter((device) => device.kind === "plugin")
    .map((device) => ({ id: device.id, name: device.name, vendor: null, version: null, format: "VST3", path: device.path ?? "", bundle: true, modifiedAtMs: null, scanState: device.path && missingPaths.has(device.path) ? "quarantined" : "validated" } as PluginEntry));
  const visiblePlugins = persistedPlugins.length ? persistedPlugins : plugins.slice(0, 3);
  const loadedBypassed = session.rack.find((device) => device.kind === "plugin")?.bypassed ?? false;
  const hasSnapshotA = session.snapshots.some((snapshot) => snapshot.id === "snapshot:A");
  const hasSnapshotB = session.snapshots.some((snapshot) => snapshot.id === "snapshot:B");
  const setMacro = (macroId: string, value: number) => {
    const safeValue = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
    const macro = session.macros.find((item) => item.id === macroId);
    setSession({ ...session, macros: session.macros.map((item) => item.id === macroId ? { ...item, value: safeValue } : item) });
    if (macro?.parameterIndex != null) onSetPluginParameter(macro.parameterIndex, safeValue);
  };
  const mapMacro = (macroId: string, value: string) => setSession({ ...session, macros: session.macros.map((item) => item.id === macroId ? { ...item, parameterIndex: value === "" ? null : Number(value) } : item) });
  return (
    <div className="workspace-scroll play-view">
      <section className="play-header"><div><span className="eyebrow">LIVE SIGNAL</span><h1>Input → Tone → Output</h1></div><div className="snapshot-tabs"><button className={hasSnapshotA ? "active" : ""} onClick={() => onRecallSnapshot("A")}>A</button><button className={hasSnapshotB ? "active" : ""} onClick={() => onRecallSnapshot("B")}>B</button><button onClick={() => onCaptureSnapshot(hasSnapshotA ? "B" : "A")}>＋</button></div></section>
      <div className="signal-line" />
      <section className="rack-flow">
        <article className="rack-device input-device"><span className="device-order">IN</span><div className="device-face"><Meter value={45} /><Meter value={40} /></div><h3>Input 1</h3><small>Mono · −12.4 dB</small></article>
        {(visiblePlugins.length ? visiblePlugins : [{ id: "placeholder", name: "Add a VST3", vendor: null } as PluginEntry]).map((plugin, index) => (
          <article className={`rack-device ${plugin.scanState === "quarantined" ? "missing-dependency" : ""}`} key={plugin.id}><span className="device-order">{String(index + 1).padStart(2, "0")}</span><div className={`device-face face-${index}`}><span>{plugin.name.slice(0, 2).toUpperCase()}</span><i /></div><h3>{plugin.name}</h3><small>{plugin.scanState === "quarantined" ? "Missing dependency" : plugin.vendor ?? "VST3 discovered"}</small><div className="device-controls"><button onClick={() => onTogglePluginBypass(!loadedBypassed)}>{loadedBypassed ? "Enable" : "Bypass"}</button><button onClick={onClearPlugin}>Remove</button><strong>0.0 dB</strong></div></article>
        ))}
        <button className="add-device"><Icon name="plus" /><span>Add Device</span></button>
        <article className="rack-device output-device"><span className="device-order">OUT</span><div className="device-face"><Meter value={58} /><Meter value={51} /></div><h3>Main Out</h3><small>Safety limited</small></article>
      </section>
      {audio.plugin?.loaded && audio.plugin.parameters.length > 0 && <section className="section-card plugin-parameters"><header><div><span className="eyebrow">COMMON PARAMETER VIEW</span><h2>{audio.plugin.parameters.length} VST3 parameters</h2></div><small>Native GUI is optional; changes stay inside the isolated rack.</small></header><div className="plugin-parameter-grid">{audio.plugin.parameters.slice(0, 48).map((parameter) => <label className="plugin-parameter" key={parameter.index}><span><strong>{parameter.name || `Parameter ${parameter.index + 1}`}</strong><small>{Math.round(parameter.value * 100)}%{parameter.automatable ? " · automatable" : ""}</small></span><input type="range" min="0" max="1" step="0.001" value={parameter.value} onChange={(event) => onSetPluginParameter(parameter.index, Number(event.target.value))} /></label>)}</div>{audio.plugin.parameters.length > 48 && <small className="inspector-copy">Showing first 48 parameters; the rest remain available to the plugin.</small>}</section>}
      <section className="macro-section">
        <header><div><span className="eyebrow">MACROS</span><h2>Performance controls</h2></div><small>{session.macros.filter((macro) => macro.parameterIndex != null).length} mapped</small></header>
        <div className="macro-grid">{session.macros.map((macro) => <label className="macro" key={macro.id}><span className="knob" style={{ "--turn": `${-120 + macro.value * 240}deg` } as React.CSSProperties}><i /></span><strong>{macro.name}</strong><input type="range" min="0" max="1" step="0.001" value={macro.value} onChange={(event) => setMacro(macro.id, Number(event.target.value))} /><small>{Math.round(macro.value * 100)}%</small><select aria-label={`${macro.name} target`} value={macro.parameterIndex == null ? "" : macro.parameterIndex} onChange={(event) => mapMacro(macro.id, event.target.value)}><option value="">Unmapped</option>{audio.plugin?.parameters.map((parameter) => <option value={parameter.index} key={parameter.index}>{parameter.name || `Parameter ${parameter.index + 1}`}</option>)}</select></label>)}</div>
      </section>
      <label className="session-note"><span>Session note</span><textarea value={session.note} onChange={(event) => setSession({ ...session, note: event.target.value })} placeholder="意図、比較対象、使用場面を記録…" /></label>
    </div>
  );
}

function EmptyWorkspace({ workspace }: { workspace: Workspace }) {
  const copy: Record<Workspace, { title: string; body: string; action: string }> = {
    home: { title: "", body: "", action: "" },
    play: { title: "", body: "", action: "" },
    arrange: { title: "Timelineへ素材を置く", body: "Recording、Audio、MIDIを非破壊で配置し、同期位置を保ったまま編集します。", action: "Inboxを開く" },
    sample: { title: "音から楽器を作る", body: "Audioを切り出し、PadまたはKeyboardへ割り当て、再利用可能なInstrumentとして保存します。", action: "Audioを選択" },
    analyze: { title: "測定して、理解する", body: "Waveform、Loudness、Spectrum、PhaseをReferenceと音量を揃えて比較します。", action: "素材をAnalyze" },
    separate: { title: "StemをBackgroundで分離", body: "Originalと結果を同期試聴し、Artifactの可能性を確認してからTimelineやLibraryへ送ります。", action: "Jobを作成" },
  };
  const item = copy[workspace];
  return <div className="empty-workspace"><span className={`empty-orbit orbit-${workspace}`}><i /><b /></span><span className="eyebrow">{workspace.toUpperCase()} WORKSPACE</span><h1>{item.title}</h1><p>{item.body}</p><button className="primary"><Icon name="plus" />{item.action}</button><small>このワークスペースの処理エンジンは後続ゲートで接続されます。Scratch Sessionは維持されます。</small></div>;
}

function WorkspaceAnalyze({ analysis }: { analysis: AudioAnalysis | null }) {
  if (!analysis) {
    return <div className="empty-workspace"><span className="empty-orbit orbit-analyze"><i /><b /></span><span className="eyebrow">ANALYZE WORKSPACE</span><h1>測定して、理解する</h1><p>LibraryのRecordingsからProcessed WAVを選ぶと、音量・位相・簡易スペクトルを確認できます。</p><small>解析はオフラインで実行され、元の録音ファイルは変更されません。</small></div>;
  }
  return <div className="workspace-scroll analysis-view"><section className="play-header"><div><span className="eyebrow">ANALYSIS RESULT</span><h1>{analysis.path.split("\\").pop() ?? "Audio"}</h1></div><span className="status-tag">READ ONLY</span></section><section className="section-card waveform-card"><span className="eyebrow">WAVEFORM</span><div className="waveform-analysis">{analysis.waveform.map((value, index) => <i key={index} style={{ height: `${Math.max(4, value * 100)}%` }} />)}</div></section><section className="analysis-grid"><article className="section-card"><span className="eyebrow">LEVEL</span><h2>{analysis.rmsDb.toFixed(1)} dB RMS</h2><p>Peak {analysis.peakDb.toFixed(1)} dBFS · True peak {analysis.truePeakDb.toFixed(1)} dBFS</p></article><article className="section-card"><span className="eyebrow">DYNAMICS</span><h2>{analysis.dynamicRangeDb.toFixed(1)} dB</h2><p>{analysis.clippingSamples.toLocaleString()} clipped samples · estimated from PCM</p></article><article className="section-card"><span className="eyebrow">SPECTRUM</span><h2>{analysis.spectrumPeakHz ? `${analysis.spectrumPeakHz.toFixed(1)} Hz` : "—"}</h2><p>簡易スペクトルピーク</p></article><article className="section-card"><span className="eyebrow">PHASE</span><h2>{analysis.phaseCorrelation == null ? "Mono" : analysis.phaseCorrelation.toFixed(3)}</h2><p>{analysis.phaseCorrelation == null ? "ステレオ相関なし" : "Left / Right correlation"}</p></article><article className="section-card"><span className="eyebrow">TIMING</span><h2>{(analysis.durationMs / 1000).toFixed(2)} s</h2><p>{analysis.sampleRate} Hz · {analysis.channels} ch · {analysis.bitsPerSample} bit</p></article></section></div>;
}

function WorkspaceArrange({ session, setSession, recordings, onPlaceRecording }: { session: ScratchSession; setSession: (value: ScratchSession) => void; recordings: RecordingAsset[]; onPlaceRecording: (recording: RecordingAsset) => void }) {
  const timelineEnd = Math.max(10_000, ...session.timeline.map((clip) => clip.startMs + clip.durationMs));
  const toggleTrack = (id: string, field: "muted" | "solo") => setSession({ ...session, tracks: session.tracks.map((track) => track.id === id ? { ...track, [field]: !track[field] } : track) });
  const updateTrack = (id: string, field: "gainDb" | "pan", value: number) => setSession({ ...session, tracks: session.tracks.map((track) => track.id !== id ? track : { ...track, [field]: field === "gainDb" ? Math.max(-90, Math.min(24, value)) : Math.max(-1, Math.min(1, value)) }) });
  const addTrack = () => {
    const name = window.prompt("Track name", `Track ${session.tracks.length + 1}`)?.trim();
    if (!name) return;
    setSession({ ...session, tracks: [...session.tracks, { id: `track:${Date.now()}`, name: name.slice(0, 80), gainDb: 0, pan: 0, muted: false, solo: false }] });
  };
  return <div className="workspace-scroll arrange-view"><section className="play-header"><div><span className="eyebrow">NON-DESTRUCTIVE TIMELINE</span><h1>Arrange ideas without moving sources</h1></div><span className="status-tag">{session.timeline.length} CLIPS · {session.tracks.length} TRACKS</span></section><section className="section-card track-mixer"><header><div><span className="eyebrow">TRACK MIXER</span><h2>Shared lanes and safe mix state</h2></div><button className="text-button" onClick={addTrack}>Add track</button></header><div className="track-mixer-grid">{session.tracks.map((track) => <div className={`track-mixer-row ${track.muted ? "muted" : ""}`} key={track.id}><strong>{track.name}</strong><label><span>Gain</span><input type="number" min="-90" max="24" step="0.5" value={track.gainDb} onChange={(event) => updateTrack(track.id, "gainDb", Number(event.target.value))} /></label><label><span>Pan</span><input type="number" min="-1" max="1" step="0.05" value={track.pan} onChange={(event) => updateTrack(track.id, "pan", Number(event.target.value))} /></label><button className="text-button" onClick={() => toggleTrack(track.id, "muted")}>{track.muted ? "Unmute" : "Mute"}</button><button className="text-button" onClick={() => toggleTrack(track.id, "solo")}>{track.solo ? "Unsolo" : "Solo"}</button></div>)}</div></section><section className="section-card timeline-card"><div className="timeline-ruler"><span>00:00</span><span>{(timelineEnd / 1000).toFixed(1)} s</span></div><div className="timeline-lane">{session.timeline.length === 0 && <small>InboxのRecordingを右側からTimelineへ配置できます。</small>}{session.timeline.map((clip) => { const track = session.tracks.find((item) => item.id === clip.trackId); return <article className={`timeline-clip ${clip.muted || track?.muted ? "muted" : ""}`} key={clip.id} style={{ left: `${(clip.startMs / timelineEnd) * 100}%`, width: `${Math.max(8, (clip.durationMs / timelineEnd) * 100)}%` }}><strong>{clip.name}</strong><small>{track?.name ?? "Main"} · {clip.gainDb.toFixed(1)} dB · {clip.muted ? "Muted" : "Source linked"}</small></article>; })}</div></section><section className="section-card arrange-sources"><header><div><span className="eyebrow">INBOX SOURCES</span><h2>素材を配置</h2></div><small>元ファイルは変更されません</small></header>{recordings.length === 0 ? <p className="inspector-copy">まだ録音がありません。</p> : recordings.slice(0, 12).map((recording) => <div className="source-row" key={recording.id}><div><strong>{recording.name}</strong><small>{recording.state} · {recording.samplesWritten.toLocaleString()} samples</small></div><button className="text-button" onClick={() => onPlaceRecording(recording)}>Place</button></div>)}</section></div>;
}

function WorkspaceSample({ session, recordings, onCreateSamplePad, onPreviewPad }: { session: ScratchSession; recordings: RecordingAsset[]; onCreateSamplePad: (recording: RecordingAsset) => void; onPreviewPad: (pad: ScratchSession["samplePads"][number]) => void }) {
  const pads = Array.from({ length: 16 }, (_, index) => session.samplePads[index] ?? null);
  const keyboardKeys = ["Z", "S", "X", "D", "C", "V", "G", "B", "H", "N", "J", "M"];
  return <div className="workspace-scroll sample-view"><section className="play-header"><div><span className="eyebrow">SAMPLE INSTRUMENT</span><h1>Audio → Pad / Keyboard</h1></div><span className="status-tag">SOURCE MAPPING</span></section><section className="section-card pad-card"><header><div><span className="eyebrow">PADS</span><h2>{session.samplePads.length} mapped</h2></div><small>Playback engine follows this mapping gate</small></header><div className="pad-grid">{pads.map((pad, index) => <button className={`sample-pad ${pad ? "filled" : "empty"}`} key={pad?.id ?? `empty-${index}`}><strong>{pad?.name ?? `Pad ${index + 1}`}</strong><small>{pad ? `MIDI ${pad.midiKey}` : "Empty"}</small></button>)}</div></section><section className="section-card sample-sources"><header><div><span className="eyebrow">SOURCES</span><h2>録音をPadへ割り当てる</h2></div><small>元ファイルは変更されません</small></header>{recordings.length === 0 ? <p className="inspector-copy">Inboxに録音がありません。</p> : recordings.slice(0, 12).map((recording) => <div className="source-row" key={recording.id}><div><strong>{recording.name}</strong><small>{recording.state} · {recording.samplesWritten.toLocaleString()} samples</small></div><button className="text-button" onClick={() => onCreateSamplePad(recording)}>Map to Pad</button></div>)}</section></div>;
}

function MidiDevices({ probe, onRefresh }: { probe: MidiProbe; onRefresh: () => void }) {
  return <section className="section-card midi-card"><header><div><span className="eyebrow">MIDI DEVICES</span><h2>Input / Output ports</h2></div><button className="text-button" onClick={onRefresh}>Refresh</button></header><div className="midi-port-grid"><div><span className="eyebrow">INPUTS</span>{probe.inputs.length ? probe.inputs.map((name) => <div className="midi-port" key={`in:${name}`}><i className="midi-led" /><strong>{name}</strong></div>) : <small className="inspector-copy">No MIDI input is visible.</small>}</div><div><span className="eyebrow">OUTPUTS</span>{probe.outputs.length ? probe.outputs.map((name) => <div className="midi-port" key={`out:${name}`}><i className="midi-led output" /><strong>{name}</strong></div>) : <small className="inspector-copy">No MIDI output is visible.</small>}</div></div><small className="midi-message">{probe.message}</small></section>;
}

function MidiMonitor({ probe, audio, onOpen, onClose, onPanic }: { probe: MidiProbe; audio: AudioStatus; onOpen: (name: string) => void; onClose: () => void; onPanic: () => void }) {
  return <section className="section-card midi-monitor"><header><div><span className="eyebrow">MIDI MONITOR</span><h2>{audio.midiInputActive ? "Listening" : "Input is closed"}</h2></div><div><button className="text-button danger" onClick={onPanic}>Panic</button><button className="text-button" disabled={!audio.midiInputActive} onClick={onClose}>Close</button></div></header>{probe.inputs.length === 0 ? <p className="inspector-copy">No MIDI input port is visible to Windows.</p> : probe.inputs.map((name) => <div className="midi-monitor-row" key={name}><strong>{name}</strong><button className="text-button" disabled={audio.midiInputActive} onClick={() => onOpen(name)}>Open</button></div>)}<small className="midi-message">Messages {audio.midiMessages} · Last note {audio.lastMidiNote == null ? "—" : audio.lastMidiNote} · Pads {audio.midiPadMappings} · Triggers {audio.midiPadTriggers}</small><small className="inspector-copy">Unmapped MIDI notes use the bounded offline sine instrument; mapped pads take priority.</small></section>;
}

function SamplePadEditor({ session, setSession }: { session: ScratchSession; setSession: (value: ScratchSession) => void }) {
  if (!session.samplePads.length) return null;
  const updateRange = (id: string, field: "startMs" | "endMs", value: number) => {
    const safeValue = Math.max(0, Math.round(Number.isFinite(value) ? value : 0));
    setSession({ ...session, samplePads: session.samplePads.map((pad) => {
      if (pad.id !== id) return pad;
      const startMs = field === "startMs" ? safeValue : pad.startMs;
      const endMs = field === "endMs" ? Math.max(1, safeValue) : pad.endMs;
      return field === "startMs"
        ? { ...pad, startMs, endMs: Math.max(endMs, startMs + 1) }
        : { ...pad, startMs: Math.min(startMs, Math.max(0, endMs - 1)), endMs };
    }) });
  };
  const updatePadValue = (id: string, field: "gainDb", value: number) => setSession({ ...session, samplePads: session.samplePads.map((pad) => pad.id === id ? { ...pad, [field]: Math.max(-90, Math.min(24, Number.isFinite(value) ? value : 0)) } : pad) });
  const togglePadLoop = (id: string) => setSession({ ...session, samplePads: session.samplePads.map((pad) => pad.id === id ? { ...pad, loopEnabled: !pad.loopEnabled } : pad) });
  const removePad = (id: string) => setSession({ ...session, samplePads: session.samplePads.filter((pad) => pad.id !== id) });
  return <section className="section-card sample-editor"><header><div><span className="eyebrow">SLICE RANGES</span><h2>Non-destructive pad regions</h2></div><small>Source files remain untouched</small></header>{session.samplePads.map((pad) => <div className="sample-edit-row" key={pad.id}><div className="sample-edit-name"><strong>{pad.name}</strong><small>MIDI {pad.midiKey} · {pad.endMs - pad.startMs} ms</small></div><label><span>Start</span><input type="number" min="0" step="1" value={pad.startMs} onChange={(event) => updateRange(pad.id, "startMs", Number(event.target.value))} /></label><label><span>End</span><input type="number" min="1" step="1" value={pad.endMs} onChange={(event) => updateRange(pad.id, "endMs", Number(event.target.value))} /></label><label><span>Gain dB</span><input type="number" min="-90" max="24" step="0.5" value={pad.gainDb} onChange={(event) => updatePadValue(pad.id, "gainDb", Number(event.target.value))} /></label><button className="text-button" onClick={() => togglePadLoop(pad.id)}>{pad.loopEnabled ? "Loop on" : "Loop"}</button><button className="text-button danger" onClick={() => removePad(pad.id)}>Remove</button></div>)}</section>;
}

function SamplePreviewControls({ session, playingId, onPreview, onStop }: { session: ScratchSession; playingId: string | null; onPreview: (pad: ScratchSession["samplePads"][number]) => void; onStop: () => void }) {
  if (!session.samplePads.length) return null;
  return <section className="section-card sample-preview"><header><div><span className="eyebrow">PREVIEW BUS</span><h2>Audition mapped regions</h2></div><button className="text-button" disabled={!playingId} onClick={onStop}>Stop</button></header>{session.samplePads.map((pad) => <div className="sample-preview-row" key={pad.id}><div><strong>{pad.name}</strong><small>MIDI {pad.midiKey} · {pad.startMs}–{pad.endMs} ms</small></div><button className={`text-button ${playingId === pad.id ? "active" : ""}`} onClick={() => onPreview(pad)}>{playingId === pad.id ? "Playing" : "Preview"}</button></div>)}</section>;
}

function TimelineEditor({ session, setSession }: { session: ScratchSession; setSession: (value: ScratchSession) => void }) {
  if (!session.timeline.length) return null;
  const updateClip = (id: string, field: "startMs" | "durationMs" | "gainDb" | "fadeInMs" | "fadeOutMs" | "pan", value: number) => {
    const safeValue = Number.isFinite(value) ? value : 0;
    setSession({ ...session, timeline: session.timeline.map((clip) => {
      if (clip.id !== id) return clip;
      if (field === "gainDb") return { ...clip, gainDb: Math.max(-90, Math.min(24, safeValue)) };
      if (field === "durationMs") return { ...clip, durationMs: Math.max(1, Math.round(safeValue)) };
      if (field === "fadeInMs") return { ...clip, fadeInMs: Math.max(0, Math.min(clip.durationMs, Math.round(safeValue))) };
      if (field === "fadeOutMs") return { ...clip, fadeOutMs: Math.max(0, Math.min(clip.durationMs, Math.round(safeValue))) };
      if (field === "pan") return { ...clip, pan: Math.max(-1, Math.min(1, safeValue)) };
      return { ...clip, startMs: Math.max(0, Math.round(safeValue)) };
    }) });
  };
  const toggleMuted = (id: string) => setSession({ ...session, timeline: session.timeline.map((clip) => clip.id === id ? { ...clip, muted: !clip.muted } : clip) });
  const removeClip = (id: string) => setSession({ ...session, timeline: session.timeline.filter((clip) => clip.id !== id) });
  return <section className="section-card timeline-editor"><header><div><span className="eyebrow">CLIP INSPECTOR</span><h2>Non-destructive edits</h2></div><small>Source WAVs remain unchanged</small></header>{session.timeline.map((clip) => <div className={`timeline-edit-row ${clip.muted ? "muted" : ""}`} key={clip.id}><div className="timeline-edit-name"><strong>{clip.name}</strong><small>{clip.assetPath}</small></div><label><span>Start ms</span><input type="number" min="0" step="1" value={clip.startMs} onChange={(event) => updateClip(clip.id, "startMs", Number(event.target.value))} /></label><label><span>Length ms</span><input type="number" min="1" step="1" value={clip.durationMs} onChange={(event) => updateClip(clip.id, "durationMs", Number(event.target.value))} /></label><label><span>Gain dB</span><input type="number" min="-90" max="24" step="0.5" value={clip.gainDb} onChange={(event) => updateClip(clip.id, "gainDb", Number(event.target.value))} /></label><button className="text-button" onClick={() => toggleMuted(clip.id)}>{clip.muted ? "Unmute" : "Mute"}</button><button className="text-button danger" onClick={() => removeClip(clip.id)}>Remove</button></div>)}</section>;
}

function TimelineClipInspector({ session, setSession }: { session: ScratchSession; setSession: (value: ScratchSession) => void }) {
  if (!session.timeline.length) return null;
  const update = (id: string, field: "startMs" | "durationMs" | "sourceInMs" | "sourceOutMs" | "gainDb" | "fadeInMs" | "fadeOutMs" | "pan", value: number) => {
    const safeValue = Number.isFinite(value) ? value : 0;
    setSession({ ...session, timeline: session.timeline.map((clip) => {
      if (clip.id !== id) return clip;
      if (field === "startMs") return { ...clip, startMs: Math.max(0, Math.round(safeValue)) };
      if (field === "durationMs") return { ...clip, durationMs: Math.max(1, Math.round(safeValue)) };
      if (field === "sourceInMs") return { ...clip, sourceInMs: Math.max(0, Math.round(safeValue)) };
      if (field === "sourceOutMs") return { ...clip, sourceOutMs: Math.max(0, Math.round(safeValue)) };
      if (field === "gainDb") return { ...clip, gainDb: Math.max(-90, Math.min(24, safeValue)) };
      if (field === "fadeInMs") return { ...clip, fadeInMs: Math.max(0, Math.min(clip.durationMs, Math.round(safeValue))) };
      if (field === "fadeOutMs") return { ...clip, fadeOutMs: Math.max(0, Math.min(clip.durationMs, Math.round(safeValue))) };
      return { ...clip, pan: Math.max(-1, Math.min(1, safeValue)) };
    }) });
  };
  const setTrack = (id: string, trackId: string) => setSession({ ...session, timeline: session.timeline.map((clip) => clip.id === id ? { ...clip, trackId } : clip) });
  const toggleMute = (id: string) => setSession({ ...session, timeline: session.timeline.map((clip) => clip.id === id ? { ...clip, muted: !clip.muted } : clip) });
  const toggleLoop = (id: string) => setSession({ ...session, timeline: session.timeline.map((clip) => clip.id === id ? { ...clip, loopEnabled: !clip.loopEnabled } : clip) });
  const duplicate = (id: string) => {
    const index = session.timeline.findIndex((clip) => clip.id === id);
    if (index < 0) return;
    const clip = session.timeline[index];
    const copy = { ...clip, id: `${clip.id}:copy:${Date.now()}`, name: `${clip.name} copy`, startMs: clip.startMs + clip.durationMs };
    const timeline = [...session.timeline];
    timeline.splice(index + 1, 0, copy);
    setSession({ ...session, timeline });
  };
  const split = (id: string) => {
    const index = session.timeline.findIndex((clip) => clip.id === id);
    if (index < 0) return;
    const clip = session.timeline[index];
    const firstDuration = Math.floor(clip.durationMs / 2);
    if (firstDuration < 1) return;
    const secondDuration = clip.durationMs - firstDuration;
    const sourceEnd = clip.sourceOutMs || clip.sourceInMs + clip.durationMs;
    const sourceSplit = Math.min(sourceEnd, clip.sourceInMs + firstDuration);
    const secondSourceOut = clip.loopEnabled ? clip.sourceOutMs : (clip.sourceOutMs > 0 && sourceEnd > sourceSplit ? sourceEnd : 0);
    const first = { ...clip, durationMs: firstDuration, sourceOutMs: clip.loopEnabled ? clip.sourceOutMs : sourceSplit };
    const second = { ...clip, id: `${clip.id}:split:${Date.now()}`, name: `${clip.name} 2`, startMs: clip.startMs + firstDuration, durationMs: secondDuration, sourceInMs: clip.loopEnabled ? clip.sourceInMs : sourceSplit, sourceOutMs: secondSourceOut };
    const timeline = [...session.timeline];
    timeline.splice(index, 1, first, second);
    setSession({ ...session, timeline });
  };
  const remove = (id: string) => setSession({ ...session, timeline: session.timeline.filter((clip) => clip.id !== id) });
  return <section className="section-card timeline-editor"><header><div><span className="eyebrow">CLIP INSPECTOR</span><h2>Non-destructive edits</h2></div><small>Source WAVs remain unchanged</small></header>{session.timeline.map((clip) => <div className={`timeline-edit-row timeline-edit-row-expanded ${clip.muted ? "muted" : ""}`} key={clip.id}><div className="timeline-edit-name"><strong>{clip.name}</strong><small>{clip.assetPath}</small></div><label><span>Track</span><select value={clip.trackId} onChange={(event) => setTrack(clip.id, event.target.value)}>{session.tracks.map((track) => <option value={track.id} key={track.id}>{track.name}</option>)}</select></label><label><span>Start ms</span><input type="number" min="0" value={clip.startMs} onChange={(event) => update(clip.id, "startMs", Number(event.target.value))} /></label><label><span>Length ms</span><input type="number" min="1" value={clip.durationMs} onChange={(event) => update(clip.id, "durationMs", Number(event.target.value))} /></label><label><span>Source in</span><input type="number" min="0" value={clip.sourceInMs} onChange={(event) => update(clip.id, "sourceInMs", Number(event.target.value))} /></label><label><span>Source out</span><input type="number" min="0" value={clip.sourceOutMs} onChange={(event) => update(clip.id, "sourceOutMs", Number(event.target.value))} /></label><label><span>Gain dB</span><input type="number" min="-90" max="24" step="0.5" value={clip.gainDb} onChange={(event) => update(clip.id, "gainDb", Number(event.target.value))} /></label><label><span>Fade in</span><input type="number" min="0" value={clip.fadeInMs} onChange={(event) => update(clip.id, "fadeInMs", Number(event.target.value))} /></label><label><span>Fade out</span><input type="number" min="0" value={clip.fadeOutMs} onChange={(event) => update(clip.id, "fadeOutMs", Number(event.target.value))} /></label><label><span>Pan</span><input type="number" min="-1" max="1" step="0.05" value={clip.pan} onChange={(event) => update(clip.id, "pan", Number(event.target.value))} /></label><button className="text-button" onClick={() => toggleLoop(clip.id)}>{clip.loopEnabled ? "Loop on" : "Loop"}</button><button className="text-button" onClick={() => duplicate(clip.id)}>Duplicate</button><button className="text-button" onClick={() => split(clip.id)}>Split</button><button className="text-button" onClick={() => toggleMute(clip.id)}>{clip.muted ? "Unmute" : "Mute"}</button><button className="text-button danger" onClick={() => remove(clip.id)}>Remove</button></div>)}</section>;
}

function TimelineRenderControls({ session, result, stems, message, onRender, onRenderStems, onPreview, onStop, previewing }: { session: ScratchSession; result: RenderResult | null; stems: RenderResult[]; message: string; onRender: (options: RenderOptions) => void; onRenderStems: (options: RenderOptions) => void; onPreview: () => void; onStop: () => void; previewing: boolean }) {
  const [rangeStartMs, setRangeStartMs] = useState(0);
  const [rangeEndMs, setRangeEndMs] = useState("");
  const [normalize, setNormalize] = useState(false);
  const [trackId, setTrackId] = useState("master");
  const options = (): RenderOptions => ({ rangeStartMs: Math.max(0, Math.round(Number(rangeStartMs) || 0)), rangeEndMs: rangeEndMs.trim() ? Math.max(1, Math.round(Number(rangeEndMs) || 1)) : null, normalize, trackId: trackId === "master" ? null : trackId });
  const submit = () => onRender(options());
  const submitStems = () => onRenderStems(options());
  const hasAudibleClips = session.timeline.some((clip) => !clip.muted);
  return <section className="section-card timeline-render"><header><div><span className="eyebrow">OFFLINE RENDER</span><h2>Export audible timeline</h2></div><div><button className="text-button" disabled={!hasAudibleClips} onClick={submit}>Render WAV</button><button className="text-button" disabled={!hasAudibleClips || session.tracks.length < 1} onClick={submitStems}>Render stems</button>{result && <button className="text-button" onClick={previewing ? onStop : onPreview}>{previewing ? "Stop preview" : "Preview"}</button>}</div></header><p className="inspector-copy">Writes a new stereo float WAV with clip position, gain, fade, pan and mute state. Source assets are never flattened. Stem export writes one safe, independent WAV per audible track.</p><div className="render-options"><label><span>Target</span><select value={trackId} onChange={(event) => setTrackId(event.target.value)}><option value="master">Master mix</option>{session.tracks.map((track) => <option value={track.id} key={track.id}>{track.name}</option>)}</select></label><label><span>Range start ms</span><input type="number" min="0" value={rangeStartMs} onChange={(event) => setRangeStartMs(Number(event.target.value))} /></label><label><span>Range end ms</span><input type="number" min="1" placeholder="Timeline end" value={rangeEndMs} onChange={(event) => setRangeEndMs(event.target.value)} /></label><label className="render-normalize"><input type="checkbox" checked={normalize} onChange={(event) => setNormalize(event.target.checked)} /><span>Normalize to -0.2 dBFS</span></label></div>{result ? <div className="render-result"><strong>{result.durationMs / 1000}s · {result.clipCount} clips · {result.trackId ? "track export" : "master mix"} · {result.normalized ? "normalized" : "master gain"}</strong><small>{result.rangeStartMs}–{result.rangeEndMs} ms</small><code>{result.path}</code></div> : <small className="render-message">{message}</small>}{stems.length > 0 && <div className="stem-results"><strong>{stems.length} track stems ready</strong>{stems.map((stem) => <div className="stem-result" key={stem.id}><span>{session.tracks.find((track) => track.id === stem.trackId)?.name ?? stem.trackId ?? "Track"}</span><small>{stem.durationMs / 1000}s · {stem.clipCount} clips</small><code>{stem.path}</code></div>)}</div>}</section>;
}

function notesFromMidiEvents(events: MidiEvent[]): MidiNote[] {
  const active = new Map<string, MidiEvent[]>();
  const notes: MidiNote[] = [];
  const ordered = [...events].sort((left, right) => left.timeMs - right.timeMs);
  const finish = (start: MidiEvent, endMs: number) => notes.push({
    id: `midi-note:${notes.length}`,
    note: start.note,
    startMs: Math.max(0, Math.round(start.timeMs)),
    durationMs: Math.max(1, Math.round(endMs - start.timeMs)),
    velocity: Math.max(1, Math.min(127, start.velocity)),
    channel: Math.max(1, Math.min(16, start.channel)),
  });
  for (const event of ordered) {
    const key = `${event.channel}:${event.note}`;
    const kind = event.status & 0xf0;
    if (kind === 0x90 && event.velocity > 0) {
      active.set(key, [...(active.get(key) ?? []), event]);
    } else if (kind === 0x80 || kind === 0x90) {
      const stack = active.get(key);
      const start = stack?.pop();
      if (start) finish(start, event.timeMs);
      if (!stack?.length) active.delete(key);
    }
  }
  const endMs = ordered.reduce((latest, event) => Math.max(latest, event.timeMs), 0) + 100;
  for (const stack of active.values()) for (const start of stack) finish(start, endMs);
  return notes.sort((left, right) => left.startMs - right.startMs || left.note - right.note);
}

function MidiClipEditor({ session, setSession, recordings }: { session: ScratchSession; setSession: (value: ScratchSession) => void; recordings: RecordingAsset[] }) {
  const [message, setMessage] = useState("Recorded MIDI sidecars can be imported as editable clips.");
  const [exportMessage, setExportMessage] = useState("");
  const importRecording = async (recording: RecordingAsset) => {
    if (!recording.midiPath) return;
    const events = await readMidiEvents(recording.midiPath);
    const notes = notesFromMidiEvents(events);
    if (!notes.length) {
      setMessage("No note-on/note-off pairs were found in that MIDI sidecar.");
      return;
    }
    const startMs = Math.max(0, ...session.timeline.map((clip) => clip.startMs + clip.durationMs), ...session.midiClips.map((clip) => clip.startMs + clip.durationMs));
    const durationMs = Math.max(1, ...notes.map((note) => note.startMs + note.durationMs));
    const clip: MidiClip = { id: `midi:${recording.id}`, name: recording.name, startMs, durationMs, notes, muted: false };
    setSession({ ...session, midiClips: [...session.midiClips.filter((item) => item.id !== clip.id), clip], workspace: "arrange" });
    setMessage(`${notes.length} notes imported from ${recording.name}.`);
  };
  const updateNote = (clipId: string, noteId: string, field: keyof Pick<MidiNote, "note" | "startMs" | "durationMs" | "velocity" | "channel">, value: number) => {
    const safeValue = Number.isFinite(value) ? Math.round(value) : 0;
    setSession({ ...session, midiClips: session.midiClips.map((clip) => clip.id !== clipId ? clip : { ...clip, notes: clip.notes.map((note) => note.id !== noteId ? note : { ...note, [field]: field === "note" || field === "velocity" || field === "channel" ? Math.max(field === "channel" ? 1 : 0, Math.min(field === "channel" ? 16 : 127, safeValue)) : Math.max(field === "durationMs" ? 1 : 0, safeValue) }) }) });
  };
  const removeNote = (clipId: string, noteId: string) => setSession({ ...session, midiClips: session.midiClips.map((clip) => clip.id !== clipId ? clip : { ...clip, notes: clip.notes.filter((note) => note.id !== noteId) }) });
  const removeClip = (clipId: string) => setSession({ ...session, midiClips: session.midiClips.filter((clip) => clip.id !== clipId) });
  const exportSessionMidi = async () => {
    const result = await exportMidi();
    setExportMessage(result ? `${result.noteCount} notes exported: ${result.path}` : "MIDI export failed; the session remains unchanged.");
  };
  return <section className="section-card midi-clip-editor"><header><div><span className="eyebrow">MIDI CLIPS</span><h2>Basic piano-roll editing</h2></div><div><button className="text-button" disabled={!session.midiClips.some((clip) => !clip.muted && clip.notes.length)} onClick={() => void exportSessionMidi()}>Export .mid</button><small>{message}</small></div></header>{exportMessage && <small className="render-message">{exportMessage}</small>}<div className="midi-import-list">{recordings.filter((recording) => recording.midiPath).slice(0, 8).map((recording) => <button className="text-button" key={recording.id} onClick={() => void importRecording(recording)}>Import {recording.name}</button>)}{!recordings.some((recording) => recording.midiPath) && <small className="inspector-copy">Quick Record a MIDI input to create an editable sidecar.</small>}</div>{session.midiClips.map((clip) => <article className={`midi-clip-card ${clip.muted ? "muted" : ""}`} key={clip.id}><header><div><strong>{clip.name}</strong><small>{clip.notes.length} notes · {clip.durationMs} ms</small></div><button className="text-button danger" onClick={() => removeClip(clip.id)}>Remove</button></header><div className="piano-roll" aria-label={`${clip.name} piano roll`}>{clip.notes.map((note) => <i key={note.id} style={{ left: `${Math.min(98, (note.startMs / Math.max(1, clip.durationMs)) * 100)}%`, width: `${Math.max(1, (note.durationMs / Math.max(1, clip.durationMs)) * 100)}%`, top: `${((127 - note.note) / 128) * 100}%` }} />)}</div><div className="midi-note-list">{clip.notes.slice(0, 64).map((note) => <div className="midi-note-row" key={note.id}><label><span>Note</span><input type="number" min="0" max="127" value={note.note} onChange={(event) => updateNote(clip.id, note.id, "note", Number(event.target.value))} /></label><label><span>Start</span><input type="number" min="0" value={note.startMs} onChange={(event) => updateNote(clip.id, note.id, "startMs", Number(event.target.value))} /></label><label><span>Length</span><input type="number" min="1" value={note.durationMs} onChange={(event) => updateNote(clip.id, note.id, "durationMs", Number(event.target.value))} /></label><label><span>Velocity</span><input type="number" min="1" max="127" value={note.velocity} onChange={(event) => updateNote(clip.id, note.id, "velocity", Number(event.target.value))} /></label><label><span>Channel</span><input type="number" min="1" max="16" value={note.channel} onChange={(event) => updateNote(clip.id, note.id, "channel", Number(event.target.value))} /></label><button className="text-button danger" onClick={() => removeNote(clip.id, note.id)}>×</button></div>)}</div>{clip.notes.length > 64 && <small className="inspector-copy">Showing first 64 notes; edits remain bounded and persisted.</small>}</article>)}</section>;
}

function ReferenceCompare({ analysis, recordings, references, referenceId, onSelect, onPreview, onStop, onSyncPreview, onToggleLoop, previewingId, syncPreviewing, loopPreview }: { analysis: AudioAnalysis | null; recordings: RecordingAsset[]; references: Record<string, AudioAnalysis>; referenceId: string | null; onSelect: (recording: RecordingAsset) => void; onPreview: (recording: RecordingAsset) => void; onStop: () => void; onSyncPreview: () => void; onToggleLoop: () => void; previewingId: string | null; syncPreviewing: boolean; loopPreview: boolean }) {
  const reference = recordings.find((recording) => recording.id === referenceId) ?? null;
  const current = recordings.find((recording) => (recording.processedPath ?? recording.rawPath) === analysis?.path) ?? null;
  const comparison = analysis && reference ? compareAnalyses(analysis, references[reference.id] ?? analysis) : null;
  return <section className="section-card reference-card"><header><div><span className="eyebrow">REFERENCE COMPARE</span><h2>Loudness-matched read-only view</h2></div><div><span className="status-tag">OFFLINE</span>{current && <button className="text-button" onClick={previewingId === current.id ? onStop : () => onPreview(current)}>{previewingId === current.id ? "Stop current" : "Preview current"}</button>}{current && reference && <button className="text-button" onClick={syncPreviewing ? onStop : onSyncPreview}>{syncPreviewing ? "Stop sync" : "Sync preview"}</button>}<label className="reference-loop"><input type="checkbox" checked={loopPreview} onChange={onToggleLoop} /> Loop preview</label></div></header>{!analysis ? <p className="inspector-copy">Analyze a recording first, then choose a reference.</p> : <><div className="reference-source-list">{recordings.length === 0 ? <small className="inspector-copy">No Inbox recordings are available.</small> : recordings.slice(0, 8).map((recording) => <div className={`reference-source-row ${recording.id === referenceId ? "active" : ""}`} key={recording.id}><button className="reference-source" onClick={() => onSelect(recording)}><strong>{recording.name}</strong><small>{recording.state} · {recording.samplesWritten.toLocaleString()} samples</small></button><button className="text-button" onClick={previewingId === recording.id ? onStop : () => onPreview(recording)}>{previewingId === recording.id ? "Stop" : "Preview"}</button></div>)}</div>{comparison && <div className="comparison-grid"><div><span className="eyebrow">RMS DELTA</span><strong>{comparison.rmsDeltaDb >= 0 ? "+" : ""}{comparison.rmsDeltaDb.toFixed(1)} dB</strong></div><div><span className="eyebrow">PEAK DELTA</span><strong>{comparison.peakDeltaDb >= 0 ? "+" : ""}{comparison.peakDeltaDb.toFixed(1)} dB</strong></div><div><span className="eyebrow">MATCH GAIN</span><strong>{comparison.loudnessMatchGainDb >= 0 ? "+" : ""}{comparison.loudnessMatchGainDb.toFixed(1)} dB</strong></div><div><span className="eyebrow">DURATION</span><strong>{comparison.durationDeltaMs >= 0 ? "+" : ""}{(comparison.durationDeltaMs / 1000).toFixed(2)} s</strong></div></div>}</>}</section>;
}

function ReferenceSuggestion({ analysis, recordings, references, referenceId, session, setSession, onSelect, onPreview, onStop, onSyncPreview, onToggleLoop, previewingId, syncPreviewing, loopPreview }: { analysis: AudioAnalysis | null; recordings: RecordingAsset[]; references: Record<string, AudioAnalysis>; referenceId: string | null; session: ScratchSession; setSession: (value: ScratchSession) => void; onSelect: (recording: RecordingAsset) => void; onPreview: (recording: RecordingAsset) => void; onStop: () => void; onSyncPreview: () => void; onToggleLoop: () => void; previewingId: string | null; syncPreviewing: boolean; loopPreview: boolean }) {
  const reference = recordings.find((recording) => recording.id === referenceId) ?? null;
  const referenceAnalysis = reference ? references[reference.id] : null;
  const contextAllowsAnalysis = session.aiContext.includes("analysis");
  const contextAllowsSelectedClip = session.aiContext.includes("selectedClip");
  const comparison = contextAllowsAnalysis && contextAllowsSelectedClip && analysis && referenceAnalysis ? compareAnalyses(analysis, referenceAnalysis) : null;
  const targetClip = analysis ? session.timeline.find((clip) => clip.assetPath === analysis.path) : null;
  const [selected, setSelected] = useState(true);
  const [previewing, setPreviewing] = useState(false);
  const [applied, setApplied] = useState(false);
  const contextOptions = [{ id: "selectedRack", label: "Selected rack" }, { id: "parameterList", label: "Parameter list" }, { id: "analysis", label: "Analysis result" }, { id: "selectedClip", label: "Selected clip" }, { id: "project", label: "Project structure" }, { id: "userNote", label: "User note" }, { id: "snapshot", label: "Snapshot" }, { id: "previewAudio", label: "Preview audio" }, { id: "errorLog", label: "Error log" }];
  const changeKey = `${referenceId ?? "none"}:${targetClip?.id ?? "none"}:${comparison?.loudnessMatchGainDb ?? "none"}`;
  useEffect(() => {
    setSelected(true);
    setPreviewing(false);
    setApplied(false);
  }, [changeKey]);
  const proposedGain = targetClip && comparison
    ? Math.max(-90, Math.min(24, targetClip.gainDb + comparison.loudnessMatchGainDb))
    : null;
  const applySuggestion = () => {
    if (!comparison || !targetClip || proposedGain == null || !selected || session.aiPermission !== "Apply") return;
    const changeSet: AiChangeSet = {
      id: `ai:${Date.now()}`,
      createdAtMs: Date.now(),
      permission: session.aiPermission,
      target: targetClip.id,
      currentGainDb: targetClip.gainDb,
      proposedGainDb: proposedGain,
      reason: "Match the selected reference RMS without changing the source WAV.",
      expectedEffect: "A closer perceived level while clip position and source remain unchanged.",
      risk: "Low · reversible",
      context: [...session.aiContext],
      applied: true,
    };
    setSession({ ...session, timeline: session.timeline.map((clip) => clip.id === targetClip.id ? { ...clip, gainDb: proposedGain } : clip), aiHistory: [...session.aiHistory, changeSet].slice(-128) });
    setApplied(true);
  };
  const toggleContext = (id: string) => setSession({ ...session, aiContext: session.aiContext.includes(id) ? session.aiContext.filter((item) => item !== id) : [...session.aiContext, id] });
  return <>
    <section className="section-card ai-context-card">
      <header>
        <div><span className="eyebrow">AI CONTEXT CONTROL</span><h2>Reversible collaborator</h2></div>
        <label className="ai-permission"><span>Permission</span><select value={session.aiPermission} onChange={(event) => setSession({ ...session, aiPermission: event.target.value as ScratchSession["aiPermission"] })}><option>Explain</option><option>Suggest</option><option>Apply</option></select></label>
      </header>
      <p className="inspector-copy">Only the checked local context is eligible for this offline suggestion. Nothing is sent to an external provider.</p>
      <div className="ai-context-list">{contextOptions.map((item) => <label key={item.id}><input type="checkbox" checked={session.aiContext.includes(item.id)} onChange={() => toggleContext(item.id)} /><span>{item.label}</span></label>)}</div>
    </section>
    <ReferenceCompare analysis={analysis} recordings={recordings} references={references} referenceId={referenceId} onSelect={onSelect} onPreview={onPreview} onStop={onStop} onSyncPreview={onSyncPreview} onToggleLoop={onToggleLoop} previewingId={previewingId} syncPreviewing={syncPreviewing} loopPreview={loopPreview} />
    {(!contextAllowsAnalysis || !contextAllowsSelectedClip) && <section className="section-card ai-context-warning"><p className="inspector-copy">The suggestion is paused until both Analysis result and Selected clip are included in the local context.</p></section>}
    {comparison && <section className="section-card suggestion-card">
      <header><div><span className="eyebrow">AI CHANGESET · {session.aiPermission.toUpperCase()}</span><h2>{targetClip ? `Loudness match for ${targetClip.name}` : "Place a clip before applying"}</h2></div><span className={`status-tag ${applied ? "success" : ""}`}>{applied ? "APPLIED" : "PREVIEW"}</span></header>
      {targetClip && proposedGain != null ? <>
        <div className="changeset-grid"><div><span className="eyebrow">TARGET</span><strong>{targetClip.name} · Gain dB</strong></div><div><span className="eyebrow">CURRENT</span><strong>{targetClip.gainDb.toFixed(1)} dB</strong></div><div><span className="eyebrow">PROPOSED</span><strong>{proposedGain.toFixed(1)} dB</strong></div><div><span className="eyebrow">RISK</span><strong>Low · reversible</strong></div></div>
        <p className="inspector-copy">Reason: match the selected reference RMS without changing the source WAV. Expected audible effect: a closer perceived level while clip position and source remain unchanged.</p>
        <div className="changeset-actions"><label><input type="checkbox" checked={selected} onChange={(event) => setSelected(event.target.checked)} /> Apply selected change</label><button className="text-button" onClick={() => setPreviewing((value) => !value)}>{previewing ? "Previewing" : "Preview"}</button><button className="text-button" disabled={!selected || applied || session.aiPermission !== "Apply"} onClick={applySuggestion}>Apply selected</button><button className="text-button danger" disabled={applied} onClick={() => { setSelected(false); setPreviewing(false); }}>Reject</button></div>
        {session.aiPermission !== "Apply" && <small className="changeset-preview">Applying is locked in {session.aiPermission} mode. Select Apply only after reviewing this ChangeSet.</small>}
        {previewing && <small className="changeset-preview">Preview only: {comparison.loudnessMatchGainDb >= 0 ? "+" : ""}{comparison.loudnessMatchGainDb.toFixed(1)} dB would be applied. No session state changed.</small>}
      </> : <p className="inspector-copy">Place this recording on Arrange to create an explicit, reversible change target.</p>}
    </section>}
    {session.aiHistory.length > 0 && <section className="section-card ai-history-card"><header><div><span className="eyebrow">AI HISTORY</span><h2>Applied ChangeSets</h2></div><small>{session.aiHistory.length} persisted</small></header>{session.aiHistory.slice(-8).reverse().map((item) => <article className="ai-history-row" key={item.id}><div><strong>{item.target}</strong><small>{new Date(item.createdAtMs).toLocaleString("ja-JP")} · {item.permission} · {item.context.join(", ") || "no context"}</small></div><span className="status-tag success">{item.applied ? "APPLIED" : "PREVIEW"}</span><p>{item.currentGainDb.toFixed(1)} dB → {item.proposedGainDb.toFixed(1)} dB · {item.reason}</p></article>)}</section>}
  </>;
}

function WorkspaceSeparate({ recordings, results, busyId, message, previewingPath, onSeparate, onPreview, onStop, onAddToTimeline }: { recordings: RecordingAsset[]; results: SeparationResult[]; busyId: string | null; message: string; previewingPath: string | null; onSeparate: (recording: RecordingAsset) => void; onPreview: (path: string) => void; onStop: () => void; onAddToTimeline: (path: string, name: string) => void }) {
  return <div className="workspace-scroll separate-view"><section className="play-header"><div><span className="eyebrow">SEPARATE WORKSPACE</span><h1>Preserve the source, derive channel assets</h1></div><span className="status-tag">CHANNEL SPLIT FALLBACK</span></section><section className="section-card separate-card"><header><div><span className="eyebrow">OFFLINE JOB</span><h2>Stereo channel split</h2></div><small>Creates immutable Left / Right WAV assets</small></header><p className="inspector-copy">This local fallback separates stereo channels without claiming vocal or instrument stems. The original WAV is never overwritten.</p>{recordings.length === 0 ? <p className="inspector-copy">Inboxに録音がありません。</p> : recordings.slice(0, 12).map((recording) => <div className="source-row" key={recording.id}><div><strong>{recording.name}</strong><small>{recording.state} · {recording.samplesWritten.toLocaleString()} samples</small></div><button className="text-button" disabled={busyId === recording.id} onClick={() => onSeparate(recording)}>{busyId === recording.id ? "Running…" : "Split stereo"}</button></div>)}<small className="separate-message">{message}</small></section><section className="section-card separate-results"><header><div><span className="eyebrow">DERIVED ASSETS</span><h2>{results.length} completed jobs</h2></div><small>Manifest-backed provenance</small></header>{results.length === 0 ? <p className="inspector-copy">No separation result has been created yet.</p> : results.slice(0, 8).map((result) => { const sourceName = result.sourcePath.split("\\").pop() ?? "Stem"; return <article className="separation-result" key={result.id}><div><strong>{sourceName}</strong><small>{new Date(result.createdAtMs).toLocaleString("ja-JP")} · {result.state}</small></div><div className="separation-paths"><span>LEFT <code>{result.leftPath}</code><button className="text-button" onClick={() => previewingPath === result.leftPath ? onStop() : onPreview(result.leftPath)}>{previewingPath === result.leftPath ? "Stop" : "Preview"}</button><button className="text-button" onClick={() => onAddToTimeline(result.leftPath, `Left · ${sourceName}`)}>Add to Timeline</button></span><span>RIGHT <code>{result.rightPath}</code><button className="text-button" onClick={() => previewingPath === result.rightPath ? onStop() : onPreview(result.rightPath)}>{previewingPath === result.rightPath ? "Stop" : "Preview"}</button><button className="text-button" onClick={() => onAddToTimeline(result.rightPath, `Right · ${sourceName}`)}>Add to Timeline</button></span></div></article>; })}</section></div>;
}

function App() {
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [session, setSession] = useState<ScratchSession | null>(null);
  const [audio, setAudio] = useState<AudioStatus>({ state: "starting", driver: null, sampleRate: null, bufferSize: null, roundTripMs: null, recording: { active: false, directory: null, sampleRate: null, rawChannels: null, processedChannels: null, samplesWritten: 0, droppedBlocks: 0 }, midiInputs: [], midiOutputs: [], midiInputActive: false, midiMessages: 0, lastMidiNote: null, midiPadMappings: 0, midiPadTriggers: 0, inputPeak: 0, outputPeak: 0, invalidSamples: 0, message: "Audio supervisor is starting." });
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);
  const [missingPluginPaths, setMissingPluginPaths] = useState<string[]>([]);
  const [recordings, setRecordings] = useState<RecordingAsset[]>([]);
  const [separations, setSeparations] = useState<SeparationResult[]>([]);
  const [separationBusy, setSeparationBusy] = useState<string | null>(null);
  const [separationMessage, setSeparationMessage] = useState("Ready for a local stereo channel split.");
  const [separationPreviewingPath, setSeparationPreviewingPath] = useState<string | null>(null);
  const [renderResult, setRenderResult] = useState<RenderResult | null>(null);
  const [stemResults, setStemResults] = useState<RenderResult[]>([]);
  const [renderPreviewing, setRenderPreviewing] = useState(false);
  const [transportPlaying, setTransportPlaying] = useState(false);
  const [recordCountdown, setRecordCountdown] = useState<number | null>(null);
  const [renderMessage, setRenderMessage] = useState("Timeline render has not been requested.");
  const [previewPadId, setPreviewPadId] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState("Autosave remains the primary session copy.");
  const [midi, setMidi] = useState<MidiProbe>({ inputs: [], outputs: [], refreshedAtMs: 0, message: "MIDI device list has not been refreshed." });
  const [deviceProbe, setDeviceProbe] = useState<AudioDeviceProbe>({ drivers: [], midiInputs: [], midiOutputs: [], refreshedAtMs: 0, message: "Audio device list has not been refreshed." });
  const [analysis, setAnalysis] = useState<AudioAnalysis | null>(null);
  const [referenceId, setReferenceId] = useState<string | null>(null);
  const [referencePreviewingId, setReferencePreviewingId] = useState<string | null>(null);
  const [referenceSyncPreviewing, setReferenceSyncPreviewing] = useState(false);
  const [referenceLoopPreview, setReferenceLoopPreview] = useState(false);
  const [referenceAnalyses, setReferenceAnalyses] = useState<Record<string, AudioAnalysis>>({});
  const [scanMessage, setScanMessage] = useState("VST3を検出中…");
  const [librarySection, setLibrarySection] = useState("Plugins");
  const [libraryQuery, setLibraryQuery] = useState("");
  const [libraryResults, setLibraryResults] = useState<LibraryAsset[]>([]);
  const [selectedLibraryAsset, setSelectedLibraryAsset] = useState<LibraryAsset | null>(null);
  const [relatedAssets, setRelatedAssets] = useState<LibraryAsset[]>([]);
  const [commandOpen, setCommandOpen] = useState(false);
  const [focusMode, setFocusMode] = useState(false);
  const [undoStack, setUndoStack] = useState<ScratchSession[]>([]);
  const [redoStack, setRedoStack] = useState<ScratchSession[]>([]);
  const saveTimer = useRef<number | undefined>(undefined);
  const previousSession = useRef<ScratchSession | null>(null);
  const historySkip = useRef(false);

  const loadPluginIntoRack = useCallback(async (plugin: PluginEntry, parameterValues: number[] = [], bypassed = false, stateData: string | null = null) => {
    let nextAudio = await loadPlugin(plugin.path);
    if (stateData) nextAudio = await setPluginState(stateData);
    for (const [index, value] of parameterValues.entries()) {
      if (index >= (nextAudio.plugin?.parameters.length ?? 0)) break;
      nextAudio = await setPluginParameter(index, value);
    }
    if (bypassed) nextAudio = await setPluginBypassed(true);
    setAudio(nextAudio);
    setSession((current) => current ? {
      ...current,
      rack: [
        ...current.rack.filter((device) => device.kind !== "plugin"),
        { id: `plugin:${plugin.id}`, name: plugin.name, kind: "plugin", path: plugin.path, bypassed, gainDb: 0, parameterValues: nextAudio.plugin?.parameters.map((parameter) => parameter.value) ?? parameterValues, stateData: nextAudio.plugin?.stateData ?? stateData },
      ],
    } : current);
  }, []);

  const clearPluginFromRack = useCallback(async () => {
    setAudio(await clearPlugin());
    setSession((current) => current ? { ...current, rack: current.rack.filter((device) => device.kind !== "plugin") } : current);
  }, []);

  const togglePluginBypass = useCallback(async (bypassed: boolean) => {
    const nextAudio = await setPluginBypassed(bypassed);
    setAudio(nextAudio);
    setSession((current) => current ? {
      ...current,
      rack: current.rack.map((device) => device.kind === "plugin" ? { ...device, bypassed } : device),
    } : current);
  }, []);

  const setPluginParameterValue = useCallback(async (index: number, value: number) => {
    const nextAudio = await setPluginParameter(index, value);
    setAudio(nextAudio);
    const values = nextAudio.plugin?.parameters.map((parameter) => parameter.value);
    if (values) setSession((current) => current ? { ...current, rack: current.rack.map((device) => device.kind === "plugin" ? { ...device, parameterValues: values, stateData: nextAudio.plugin?.stateData ?? device.stateData } : device) } : current);
  }, []);

  const recoverAudio = useCallback(async () => {
    setAudio(await recoverAudioDevice());
  }, []);

  const selectAudioDriver = useCallback(async (driver: string, sampleRate: number, bufferSize: number) => {
    setAudio(await setAudioDriver(driver, sampleRate, bufferSize));
  }, []);

  const connectMidiInput = useCallback(async (name: string) => {
    setAudio(await openMidiInput(name));
  }, []);

  const disconnectMidiInput = useCallback(async () => {
    setAudio(await closeMidiInput());
  }, []);

  const undo = useCallback(() => {
    if (!session || undoStack.length === 0) return;
    const previous = undoStack[undoStack.length - 1];
    historySkip.current = true;
    setUndoStack(undoStack.slice(0, -1));
    setRedoStack([...redoStack, session].slice(-40));
    setSession(previous);
  }, [redoStack, session, undoStack]);

  const redo = useCallback(() => {
    if (!session || redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    historySkip.current = true;
    setRedoStack(redoStack.slice(0, -1));
    setUndoStack([...undoStack, session].slice(-40));
    setSession(next);
  }, [redoStack, session, undoStack]);

  const captureSnapshot = useCallback((slot: "A" | "B") => {
    if (!session) return;
    const id = `snapshot:${slot}`;
    const snapshot = {
      id,
      name: slot,
      createdAtMs: Date.now(),
      description: "",
      tag: null,
      parentId: null,
      masterDb: session.masterDb,
      rack: session.rack.map((device) => ({ ...device })),
      macros: session.macros.map((macro) => ({ ...macro })),
    };
    setSession({
      ...session,
      snapshots: [...session.snapshots.filter((item) => item.id !== id), snapshot],
    });
  }, [session]);

  const recallSnapshot = useCallback(async (slot: "A" | "B") => {
    if (!session) return;
    const snapshot = session.snapshots.find((item) => item.id === `snapshot:${slot}`);
    if (!snapshot) return;
    setSession({ ...session, masterDb: snapshot.masterDb, rack: snapshot.rack.map((device) => ({ ...device })), macros: snapshot.macros.map((macro) => ({ ...macro })) });
    const plugin = snapshot.rack.find((device) => device.kind === "plugin");
    if (plugin) {
      let nextAudio = plugin.stateData ? await setPluginState(plugin.stateData) : await setPluginBypassed(plugin.bypassed);
      if (plugin.stateData) nextAudio = await setPluginBypassed(plugin.bypassed);
      for (const [index, value] of plugin.parameterValues.entries()) {
        if (index >= (nextAudio.plugin?.parameters.length ?? 0)) break;
        nextAudio = await setPluginParameter(index, value);
      }
      setAudio(nextAudio);
    }
  }, [session]);

  const openRecordingAnalysis = useCallback(async (recording: RecordingAsset) => {
    const path = recording.processedPath ?? recording.rawPath;
    if (!path) return;
    setAnalysis(await analyzeAudio(path));
    setSession((current) => current ? { ...current, workspace: "analyze" } : current);
  }, []);

  const selectReference = useCallback(async (recording: RecordingAsset) => {
    const path = recording.processedPath ?? recording.rawPath;
    if (!path) return;
    setReferenceId(recording.id);
    const existing = referenceAnalyses[recording.id];
    if (existing) return;
    const next = await analyzeAudio(path);
    if (next) setReferenceAnalyses((current) => ({ ...current, [recording.id]: next }));
  }, [referenceAnalyses]);

  const previewReference = useCallback(async (recording: RecordingAsset) => {
    const path = recording.processedPath ?? recording.rawPath;
    if (!path) return;
    await stopSamplePreview();
    setAudio(await previewSample(path, 0, 0, referenceLoopPreview));
    setReferencePreviewingId(recording.id);
    setReferenceSyncPreviewing(false);
  }, [referenceLoopPreview]);

  const previewReferencePair = useCallback(async () => {
    if (!analysis || !referenceId) return;
    const reference = recordings.find((recording) => recording.id === referenceId);
    if (!reference) return;
    const referencePath = reference.processedPath ?? reference.rawPath;
    if (!referencePath) return;
    await stopSamplePreview();
    await previewSample(analysis.path, 0, 0, referenceLoopPreview);
    setAudio(await previewSample(referencePath, 0, 0, referenceLoopPreview));
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(true);
  }, [analysis, recordings, referenceId, referenceLoopPreview]);

  const stopReferencePreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(false);
  }, []);

  const runSeparation = useCallback(async (recording: RecordingAsset) => {
    const path = recording.processedPath ?? recording.rawPath;
    if (!path) return;
    setSeparationBusy(recording.id);
    setSeparationMessage("Writing Left / Right WAV assets…");
    const result = await separateChannels(path);
    setSeparationBusy(null);
    if (!result) {
      setSeparationMessage("Separation failed; the source and saved session remain unchanged.");
      return;
    }
    setSeparations((current) => [result, ...current.filter((item) => item.id !== result.id)]);
    setSeparationMessage(result.message);
  }, []);

  const previewSeparation = useCallback(async (path: string) => {
    setAudio(await previewSample(path, 0, 0));
    setSeparationPreviewingPath(path);
  }, []);

  const stopSeparationPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setSeparationPreviewingPath(null);
  }, []);

  const addSeparationToTimeline = useCallback(async (path: string, name: string) => {
    if (!session) return;
    const metrics = await analyzeAudio(path);
    const durationMs = metrics?.durationMs ?? 1_000;
    const startMs = Math.max(0, ...session.timeline.map((clip) => clip.startMs + clip.durationMs));
    setSession({ ...session, timeline: [...session.timeline, { id: `clip:separation:${Date.now()}`, assetPath: path, name, trackId: session.tracks[0]?.id ?? "main", startMs, durationMs, sourceInMs: 0, sourceOutMs: 0, loopEnabled: false, gainDb: 0, fadeInMs: 0, fadeOutMs: 0, pan: 0, muted: false }], workspace: "arrange" });
  }, [session]);

  const runTimelineRender = useCallback(async (options: RenderOptions) => {
    setRenderResult(null);
    setStemResults([]);
    setRenderPreviewing(false);
    setRenderMessage("Rendering a new stereo WAV…");
    const result = await renderTimeline(options);
    if (!result) {
      setRenderMessage("Render failed; source clips and the session remain unchanged.");
      return;
    }
    setRenderResult(result);
    setRenderMessage(result.message);
  }, []);

  const runTimelineStemRender = useCallback(async (options: RenderOptions) => {
    setRenderResult(null);
    setStemResults([]);
    setRenderPreviewing(false);
    setRenderMessage("Rendering independent track stems…");
    const results = await renderTimelineStems(options);
    if (!results.length) {
      setRenderMessage("Stem render failed; source clips and the session remain unchanged.");
      return;
    }
    setStemResults(results);
    setRenderMessage(`${results.length} track stems rendered without changing source clips.`);
  }, []);

  const previewTimelineRender = useCallback(async () => {
    if (!renderResult) return;
    setAudio(await previewSample(renderResult.path, 0, 0, session?.loopEnabled ?? false));
    setRenderPreviewing(true);
    setTransportPlaying(true);
  }, [renderResult, session?.loopEnabled]);

  const stopTimelinePreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setRenderPreviewing(false);
    setTransportPlaying(false);
  }, []);

  const playTransport = useCallback(async () => {
    if (!session) return;
    let result = renderResult;
    if (!result) {
      result = await renderTimeline({ rangeStartMs: 0, rangeEndMs: null, normalize: false, trackId: null });
      if (!result) return;
      setRenderResult(result);
    }
    setAudio(await previewSample(result.path, 0, 0, session.loopEnabled));
    setTransportPlaying(true);
  }, [renderResult, session]);

  const stopTransport = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setTransportPlaying(false);
    setRenderPreviewing(false);
  }, []);

  const previewSamplePad = useCallback(async (pad: ScratchSession["samplePads"][number]) => {
    const nextAudio = await previewSample(pad.assetPath, pad.startMs, pad.endMs, pad.loopEnabled, Math.pow(10, (pad.gainDb ?? 0) / 20));
    setAudio(nextAudio);
    setPreviewPadId(pad.id);
  }, []);

  const stopPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setPreviewPadId(null);
  }, []);

  const placeRecording = useCallback((recording: RecordingAsset) => {
    if (!session) return;
    const assetPath = recording.processedPath ?? recording.rawPath;
    if (!assetPath || session.timeline.some((clip) => clip.assetPath === assetPath)) return;
    const startMs = session.timeline.reduce((end, clip) => Math.max(end, clip.startMs + clip.durationMs), 0);
    const durationMs = recording.sampleRate && recording.samplesWritten
      ? Math.max(1, Math.round((recording.samplesWritten / recording.sampleRate) * 1000))
      : 1_000;
    setSession({
      ...session,
      timeline: [...session.timeline, { id: `clip:${recording.id}`, assetPath, name: recording.name, trackId: session.tracks[0]?.id ?? "main", startMs, durationMs, sourceInMs: 0, sourceOutMs: 0, loopEnabled: false, gainDb: 0, fadeInMs: 0, fadeOutMs: 0, pan: 0, muted: false }],
      workspace: "arrange",
    });
  }, [session]);

  const createSamplePad = useCallback((recording: RecordingAsset) => {
    if (!session) return;
    const assetPath = recording.processedPath ?? recording.rawPath;
    if (!assetPath || session.samplePads.some((pad) => pad.assetPath === assetPath)) return;
    const index = session.samplePads.length;
    const endMs = recording.sampleRate && recording.samplesWritten
      ? Math.max(1, Math.round((recording.samplesWritten / recording.sampleRate) * 1000))
      : 1_000;
    setSession({
      ...session,
      samplePads: [...session.samplePads, { id: `pad:${recording.id}`, name: recording.name, assetPath, startMs: 0, endMs, midiKey: 36 + index, gainDb: 0, loopEnabled: false }],
      workspace: "sample",
    });
  }, [session]);

  useEffect(() => {
    void bootstrap().then((state) => {
      setBoot(state);
      setSession(state.session);
      if (!state.safeMode) void setMasterGainDb(state.session.masterDb).then(setAudio);
      void scanVst3Folder(state.vst3Root).then((report) => {
        setPlugins(report.plugins);
        setMissingPluginPaths(state.session.rack
          .filter((device) => device.kind === "plugin" && device.path)
          .filter((device) => !report.plugins.some((plugin) => plugin.path === device.path && plugin.scanState === "validated"))
          .map((device) => device.path as string));
        setScanMessage(report.issues.length ? `${report.plugins.length}件 · ${report.issues.length}件の注意` : `${report.plugins.length}件を検出`);
        const persisted = state.session.rack.find((device) => device.kind === "plugin" && device.path);
        const restored = persisted && report.plugins.find((plugin) => plugin.path === persisted.path && plugin.scanState === "validated");
        if (restored) void loadPluginIntoRack(restored, persisted?.parameterValues ?? [], persisted?.bypassed ?? false, persisted?.stateData ?? null);
      });
    });
    void listRecordings().then(setRecordings);
    void listSeparations().then(setSeparations);
    void probeMidiDevices().then(setMidi);
    void probeAudioDevices().then(setDeviceProbe);
    const refreshAudio = () => void getAudioStatus().then(setAudio);
    refreshAudio();
    const audioPoll = window.setInterval(refreshAudio, 1000);
    return () => {
      window.clearInterval(audioPoll);
    };
  }, []);

  useEffect(() => {
    if (!session) return;
    const previous = previousSession.current;
    if (previous && JSON.stringify(previous) !== JSON.stringify(session)) {
      if (historySkip.current) historySkip.current = false;
      else {
        setUndoStack((stack) => [...stack, previous].slice(-40));
        setRedoStack([]);
      }
    }
    previousSession.current = session;
  }, [session]);

  useEffect(() => {
    if (!session) return;
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => void saveScratch({ ...session, updatedAtMs: Date.now() }), 750);
    return () => window.clearTimeout(saveTimer.current);
  }, [session]);

  useEffect(() => {
    if (!session || !boot || boot.safeMode) return;
    void configureSamplePads(session.samplePads).then(setAudio);
  }, [audio.state, boot, session?.samplePads]);

  useEffect(() => {
    if (!session) return;
    const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>(".sample-pad.filled"));
    const handlers = buttons.map((button, index) => {
      const handler = () => {
        const pad = session.samplePads[index];
        if (pad) void previewSamplePad(pad);
      };
      button.addEventListener("click", handler);
      return { button, handler };
    });
    return () => handlers.forEach(({ button, handler }) => button.removeEventListener("click", handler));
  }, [previewSamplePad, session?.samplePads]);

  useEffect(() => {
    const keyboardKeys = ["z", "s", "x", "d", "c", "v", "g", "b", "h", "n", "j", "m"];
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.tagName === "INPUT" || target?.tagName === "TEXTAREA") return;
      const index = keyboardKeys.indexOf(event.key.toLowerCase());
      const pad = index >= 0 ? session?.samplePads[index] : undefined;
      if (pad) {
        event.preventDefault();
        void previewSamplePad(pad);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [previewSamplePad, session?.samplePads]);

  const switchWorkspace = useCallback((workspace: Workspace) => {
    setSession((current) => current ? { ...current, workspace } : current);
  }, []);

  const renameSession = useCallback(() => {
    if (!session) return;
    const next = window.prompt("Scratch Session name", session.projectName ?? "Untitled Scratch");
    if (next == null) return;
    const name = next.trim().slice(0, 160);
    setSession({ ...session, projectName: name || null });
  }, [session]);

  const exportSession = useCallback(async () => {
    const result = await exportScratchSession();
    setExportMessage(result ? `Exported manifest with ${result.assetCount} collected assets: ${result.path}` : "Export failed; the current session remains safe.");
  }, []);

  const importSession = useCallback(async () => {
    const path = window.prompt("Path to a Riffra project.json manifest");
    if (!path) return;
    const imported = await importScratchSession(path.trim());
    if (!imported) {
      setExportMessage("Import failed; the current session remains safe.");
      return;
    }
    setSession(imported);
    setMissingPluginPaths([]);
    setBoot((current) => current ? { ...current, session: imported, recoveredFromGeneration: false } : current);
    setUndoStack([]);
    setRedoStack([]);
    setExportMessage(`Imported session: ${imported.projectName ?? imported.sessionId}`);
  }, []);

  const restoreRecovery = useCallback(async (fileName: string) => {
    if (!window.confirm(`Restore autosave generation ${fileName}? The current session will become the selected stable copy.`)) return;
    const restored = await restoreRecoveryGeneration(fileName);
    if (!restored) {
      setExportMessage("Recovery generation could not be restored; the current session remains safe.");
      return;
    }
    setSession(restored);
    setBoot((current) => current ? { ...current, session: restored, recoveredFromGeneration: false } : current);
    setUndoStack([]);
    setRedoStack([]);
    setExportMessage(`Restored stable generation: ${restored.projectName ?? restored.sessionId}`);
  }, []);

  const dismissRecovery = useCallback(() => {
    setBoot((current) => current ? { ...current, recoveredFromGeneration: false } : current);
    setExportMessage("Recovered session kept as the active working copy.");
  }, []);

  const selectLibraryAsset = useCallback(async (asset: LibraryAsset) => {
    setSelectedLibraryAsset(asset);
    setRelatedAssets(await relatedLibraryAssets(asset.id));
  }, []);

  const editSelectedLibraryAsset = useCallback(async () => {
    if (!selectedLibraryAsset) return;
    const tag = window.prompt("Asset tags (comma-separated)", selectedLibraryAsset.tag ?? "");
    if (tag == null) return;
    const note = window.prompt("Asset note", selectedLibraryAsset.note ?? "");
    if (note == null) return;
    const updated = await updateLibraryAsset(selectedLibraryAsset.id, tag, note);
    if (!updated) return;
    setSelectedLibraryAsset(updated);
    setLibraryResults((current) => current.map((asset) => asset.id === updated.id ? updated : asset));
  }, [selectedLibraryAsset]);

  const previewSelectedLibraryAsset = useCallback(async () => {
    const path = selectedLibraryAsset?.path;
    if (!path || !path.toLowerCase().endsWith(".wav")) return;
    setAudio(await previewSample(path, 0, 0));
    setPreviewPadId(null);
  }, [selectedLibraryAsset]);

  const toggleMute = useCallback(async () => {
    if (!session) return;
    const muted = !(session.emergencyMuted || audio.state === "muted");
    setSession({ ...session, emergencyMuted: muted });
    setAudio(await setEmergencyMute(muted));
  }, [audio.state, session]);

  const startRecordingNow = useCallback(async () => {
    const nextAudio = await startRecording();
    setAudio(nextAudio);
    setRecordings(await listRecordings());
  }, []);

  const toggleRecording = useCallback(async () => {
    if (recordCountdown !== null) {
      setRecordCountdown(null);
      return;
    }
    if (audio.recording.active) {
      setAudio(await stopRecording());
      setRecordings(await listRecordings());
      return;
    }
    const beats = session?.countInBeats ?? 0;
    if (beats > 0) {
      setRecordCountdown(beats);
      return;
    }
    await startRecordingNow();
  }, [audio.recording.active, recordCountdown, session?.countInBeats, startRecordingNow]);

  useEffect(() => {
    if (recordCountdown === null) return;
    if (recordCountdown === 0) {
      setRecordCountdown(null);
      void startRecordingNow();
      return;
    }
    const timer = window.setTimeout(() => setRecordCountdown((current) => current === null ? null : current - 1), 1_000);
    return () => window.clearTimeout(timer);
  }, [recordCountdown, startRecordingNow]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA";
      if (event.ctrlKey && event.key.toLowerCase() === "k") { event.preventDefault(); setCommandOpen((open) => !open); return; }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === "z") { event.preventDefault(); event.shiftKey ? redo() : undo(); return; }
      if (event.ctrlKey && !typing && event.key.toLowerCase() === "y") { event.preventDefault(); redo(); return; }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "m") { event.preventDefault(); void toggleMute(); return; }
      if (!typing && event.key >= "1" && event.key <= "6") switchWorkspace(workspaces[Number(event.key) - 1].id);
      if (event.key === "Escape") setCommandOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [redo, switchWorkspace, toggleMute, undo]);

  const selectedPlugin = useMemo(() => plugins[0] ?? null, [plugins]);
  const query = libraryQuery.trim().toLowerCase();
  const visiblePlugins = query ? plugins.filter((plugin) => `${plugin.name} ${plugin.vendor ?? ""} ${plugin.path}`.toLowerCase().includes(query)) : plugins;
  const visibleRecordings = query ? recordings.filter((recording) => `${recording.name} ${recording.state} ${recording.path}`.toLowerCase().includes(query)) : recordings;
  useEffect(() => {
    let active = true;
    if (!query) {
      setLibraryResults([]);
      setSelectedLibraryAsset(null);
      setRelatedAssets([]);
      return () => { active = false; };
    }
    void searchLibrary(query).then((results) => {
      if (active) setLibraryResults(results);
    });
    return () => { active = false; };
  }, [query]);
  if (!boot || !session) return <div className="boot-screen"><span className="logo-mark">R</span><strong>Riffra</strong><small>Recovering your creative memory…</small></div>;

  const isMuted = session.emergencyMuted || audio.state === "muted";
  return (
    <main className={`app-shell ${focusMode ? "focus-mode" : ""} ${isMuted ? "is-muted" : ""}`}>
      <header className="global-bar">
        <div className="brand"><span className="logo-mark">R</span><strong>RIFFRA</strong></div>
        <button className="session-title" onClick={renameSession} title="Rename Scratch Session"><span className="save-light" />{session.projectName ?? "Untitled Scratch"}<small>Auto-saved</small><Icon name="chevron" /></button>
        <div className="history-controls"><button aria-label="Undo" title="Undo (Ctrl+Z)" disabled={undoStack.length === 0} onClick={undo}>↶</button><button aria-label="Redo" title="Redo (Ctrl+Y)" disabled={redoStack.length === 0} onClick={redo}>↷</button></div>
        <nav className="workspace-tabs" aria-label="Workspace">
          {workspaces.map((item) => <button key={item.id} className={session.workspace === item.id ? "active" : ""} onClick={() => switchWorkspace(item.id)}>{item.label}<kbd>{item.key}</kbd></button>)}
        </nav>
        <button className="command-trigger" onClick={() => setCommandOpen(true)}><Icon name="search" />Search or command<kbd>Ctrl K</kbd></button>
        <button className={`engine-pill ${audio.state}`}><span />{audio.state === "ready" ? audio.driver : audio.state}<small>{audio.roundTripMs ? `${audio.roundTripMs} ms` : "Audio"}</small></button>
        <button className={`emergency-button ${isMuted ? "active" : ""}`} onClick={() => void toggleMute()}><Icon name="stop" />{isMuted ? "UNMUTE" : "MUTE"}</button>
      </header>

      <aside className="library-panel">
        <div className="panel-heading"><span>LIBRARY</span><button><Icon name="plus" /></button></div>
        <label className="panel-search"><Icon name="search" /><input aria-label="Library search" value={libraryQuery} onChange={(event) => setLibraryQuery(event.target.value)} placeholder="Search assets" /></label>
        <nav>{librarySections.map((section) => <button key={section} className={librarySection === section ? "active" : ""} onClick={() => setLibrarySection(section)}><span className={`nav-glyph glyph-${section.toLowerCase()}`} />{section}<small>{section === "Plugins" ? plugins.length : ""}</small></button>)}</nav>
        <div className="library-content">
          <span className="eyebrow">{librarySection.toUpperCase()}</span>
          {query && <section className="library-search-results"><span className="eyebrow">CROSS-ASSET SEARCH · {libraryResults.length}</span>{libraryResults.slice(0, 8).map((asset) => <button className="library-search-row" key={asset.id} onClick={() => void selectLibraryAsset(asset)}><span className="nav-glyph" /><div><strong>{asset.name}</strong><small>{asset.kind} · {asset.stability}{asset.tag ? ` · ${asset.tag}` : ""}</small></div></button>)}{libraryResults.length === 0 && <small className="library-search-empty">No indexed asset matches yet.</small>}{selectedLibraryAsset && <div className="library-asset-detail"><header><div><span className="eyebrow">ASSET MEMORY</span><strong>{selectedLibraryAsset.name}</strong></div><div><button className="text-button" disabled={!selectedLibraryAsset.path?.toLowerCase().endsWith(".wav")} onClick={() => void previewSelectedLibraryAsset()}>Preview</button><button className="text-button" onClick={() => void editSelectedLibraryAsset()}>Edit</button></div></header><small>Tag: {selectedLibraryAsset.tag ?? "—"}</small><p>{selectedLibraryAsset.note ?? "No note yet."}</p>{relatedAssets.length > 0 && <div><span className="eyebrow">RELATED</span>{relatedAssets.slice(0, 4).map((asset) => <small className="related-asset" key={asset.id}>{asset.kind} · {asset.name}</small>)}</div>}</div>}</section>}
          {librarySection === "Plugins" ? <><small className="scan-message">{visiblePlugins.length}件を表示</small>{visiblePlugins.slice(0, 12).map((plugin) => <button className="plugin-row" key={plugin.id} onClick={() => void loadPluginIntoRack(plugin)} title={`Load ${plugin.name}`}><span>{plugin.name.slice(0, 1).toUpperCase()}</span><div><strong>{plugin.name}</strong><small>{plugin.vendor ?? "VST3"}</small></div><i className={`stability ${plugin.scanState}`} /></button>)}{visiblePlugins.length === 0 && <div className="library-empty"><span>一致するVST3がありません</span><small>検索語を変えるか、VST3フォルダを確認してください。</small></div>}</> : librarySection === "Recordings" ? <>{visibleRecordings.slice(0, 12).map((recording) => <button className="plugin-row recording-row" key={recording.id} onClick={() => void openRecordingAnalysis(recording)} title={recording.path}><span>{recording.state === "completed" ? "✓" : "!"}</span><div><strong>{recording.name}</strong><small>{recording.state} · {recording.samplesWritten.toLocaleString()} samples{recording.midiPath ? " · MIDI" : ""}</small></div><i className={`stability ${recording.state === "completed" ? "validated" : "quarantined"}`} /></button>)}{visibleRecordings.length === 0 && <div className="library-empty"><span>まだ録音がありません</span><small>Quick RecordまたはTransportの録音ボタンからInboxへ保全できます。</small></div>}</> : <div className="library-empty"><span>まだ資産がありません</span><small>良い結果を保存すると、ここから再利用できます。</small></div>}
        </div>
        <button className="inbox-button" onClick={() => setLibrarySection("Recordings")}><span className="inbox-icon">↓</span><div><strong>Inbox</strong><small>{recordings.length} items</small></div></button>
      </aside>

      <section className="workspace">
        {session.workspace === "home" && <><WorkspaceHome state={boot} onWorkspace={switchWorkspace} onQuickRecord={() => void toggleRecording()} recordingActive={audio.recording.active || recordCountdown !== null} onRecoverAudioDevice={() => void recoverAudio()} onExportProject={() => void exportSession()} onImportProject={() => void importSession()} onRestoreRecovery={(fileName) => void restoreRecovery(fileName)} onDismissRecovery={dismissRecovery} exportMessage={exportMessage} /><AudioDevices probe={deviceProbe} onRefresh={() => void probeAudioDevices().then(setDeviceProbe)} /><AudioDriverPicker probe={deviceProbe} current={audio.driver} sampleRate={audio.sampleRate} bufferSize={audio.bufferSize} onSelect={(driver, sampleRate, bufferSize) => void selectAudioDriver(driver, sampleRate, bufferSize)} /><CaptureSettings session={session} setSession={setSession} /></>}
        {session.workspace === "play" && <WorkspacePlay session={session} audio={audio} plugins={plugins} missingPluginPaths={missingPluginPaths} setSession={setSession} onTogglePluginBypass={(bypassed) => void togglePluginBypass(bypassed)} onSetPluginParameter={(index, value) => void setPluginParameterValue(index, value)} onClearPlugin={() => void clearPluginFromRack()} onCaptureSnapshot={captureSnapshot} onRecallSnapshot={(slot) => void recallSnapshot(slot)} />}
        {session.workspace === "arrange" && <><WorkspaceArrange session={session} setSession={setSession} recordings={recordings} onPlaceRecording={placeRecording} /><TimelineClipInspector session={session} setSession={setSession} /><MidiClipEditor session={session} setSession={setSession} recordings={recordings} /><TimelineRenderControls session={session} result={renderResult} stems={stemResults} message={renderMessage} onRender={(options) => void runTimelineRender(options)} onRenderStems={(options) => void runTimelineStemRender(options)} onPreview={() => void previewTimelineRender()} onStop={() => void stopTimelinePreview()} previewing={renderPreviewing} /></>}
        {session.workspace === "sample" && <><WorkspaceSample session={session} recordings={recordings} onCreateSamplePad={createSamplePad} onPreviewPad={(pad) => void previewSamplePad(pad)} /><SamplePadEditor session={session} setSession={setSession} /><SamplePreviewControls session={session} playingId={previewPadId} onPreview={(pad) => void previewSamplePad(pad)} onStop={() => void stopPreview()} /><MidiDevices probe={midi} onRefresh={() => void probeMidiDevices().then(setMidi)} /><MidiMonitor probe={midi} audio={audio} onOpen={(name) => void connectMidiInput(name)} onClose={() => void disconnectMidiInput()} onPanic={() => void stopPreview()} /></>}
        {session.workspace === "analyze" && <><WorkspaceAnalyze analysis={analysis} /><ReferenceSuggestion analysis={analysis} recordings={recordings} references={referenceAnalyses} referenceId={referenceId} session={session} setSession={setSession} onSelect={(recording) => void selectReference(recording)} onPreview={(recording) => void previewReference(recording)} onStop={() => void stopReferencePreview()} onSyncPreview={() => void previewReferencePair()} onToggleLoop={() => setReferenceLoopPreview((value) => !value)} previewingId={referencePreviewingId} syncPreviewing={referenceSyncPreviewing} loopPreview={referenceLoopPreview} /></>}
        {session.workspace === "separate" && <WorkspaceSeparate recordings={recordings} results={separations} busyId={separationBusy} message={separationMessage} previewingPath={separationPreviewingPath} onSeparate={(recording) => void runSeparation(recording)} onPreview={(path) => void previewSeparation(path)} onStop={() => void stopSeparationPreview()} onAddToTimeline={(path, name) => void addSeparationToTimeline(path, name)} />}
        {!(["home", "play", "arrange", "sample", "analyze", "separate"] as Workspace[]).includes(session.workspace) && <EmptyWorkspace workspace={session.workspace} />}
      </section>

      <aside className="inspector-panel">
        <div className="panel-heading"><span>INSPECTOR</span><button onClick={() => setFocusMode(true)}>×</button></div>
        <div className="inspector-identity"><span className="inspector-art">{selectedPlugin?.name.slice(0, 2).toUpperCase() ?? "SS"}</span><div><span className="eyebrow">{selectedPlugin ? "PLUGIN" : "SESSION"}</span><h3>{selectedPlugin?.name ?? "Scratch Session"}</h3><small>{selectedPlugin?.vendor ?? "Always preserved"}</small></div></div>
        <section><header><strong>Signal</strong><Icon name="chevron" /></header><dl><div><dt>Input</dt><dd>Mono</dd></div><div><dt>Gain</dt><dd>0.0 dB</dd></div><div><dt>State</dt><dd className="safe-label">Safe</dd></div></dl></section>
        <section><header><strong>Tone engine</strong><Icon name="chevron" /></header><dl><div><dt>Rack</dt><dd className={audio.plugin?.loaded ? "safe-label" : ""}>{audio.plugin?.loaded ? "Loaded" : "Empty"}</dd></div><div><dt>VST3</dt><dd>{audio.plugin?.name ?? "—"}</dd></div><div><dt>State</dt><dd>{audio.plugin?.bypassed ? "Bypassed" : "Active"}</dd></div><div><dt>Bypassed blocks</dt><dd>{audio.plugin?.bypassedBlocks ?? 0}</dd></div></dl></section>
        <section><header><strong>Provenance</strong><Icon name="chevron" /></header><dl><div><dt>Session</dt><dd>Scratch</dd></div><div><dt>Updated</dt><dd>{new Date(session.updatedAtMs).toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" })}</dd></div></dl></section>
        <section><header><strong>Data safety</strong><Icon name="chevron" /></header><p className="inspector-copy">世代付き自動保存が有効です。現在の作業はプロジェクトへ昇格しなくても保持されます。</p><small className="path-copy">{boot.dataRoot}</small></section>
        <button className="focus-button" onClick={() => setFocusMode(!focusMode)}>{focusMode ? "Exit Focus Mode" : "Focus Mode"}</button>
      </aside>

      <footer className="transport">
        <div className="transport-left"><button className={session.loopEnabled ? "active" : ""} aria-label="Toggle loop" onClick={() => setSession({ ...session, loopEnabled: !session.loopEnabled })}><Icon name="loop" /></button><button aria-label="Previous position">◀</button><button className="play-button" aria-label={transportPlaying ? "Stop playback" : "Play"} onClick={() => void (transportPlaying ? stopTransport() : playTransport())}><Icon name={transportPlaying ? "stop" : "play"} /></button><button aria-label="Stop" onClick={() => void stopTransport()}><Icon name="stop" /></button><button className={`record-button ${audio.recording.active ? "active" : ""}`} onClick={() => void toggleRecording()} aria-label={audio.recording.active ? "Stop recording" : "Start recording"}><Icon name="record" /></button></div>
        <div className="position"><strong>001 · 01 · 000</strong><small>00:00:00.000</small></div>
        <div className="tempo"><button><strong>120.00</strong><small>BPM</small></button><button><strong>4 / 4</strong><small>TIME</small></button></div>
        <div className="transport-meter"><span>IN</span><Meter value={audio.inputPeak * 100} danger={audio.inputPeak >= 0.98} /><span>OUT</span><Meter value={audio.outputPeak * 100} danger={audio.outputPeak >= 0.98} /></div>
        <div className="master"><span>MASTER</span><strong>{session.masterDb.toFixed(1)} dB</strong><input aria-label="Master volume" type="range" min="-60" max="0" step="0.5" value={session.masterDb} onChange={(event) => { const gainDb = Number(event.target.value); setSession({ ...session, masterDb: gainDb }); void setMasterGainDb(gainDb).then(setAudio); }} /></div>
        <div className="status-line"><span className={`status-dot ${audio.recording.active || recordCountdown !== null ? "recording" : audio.state}`} />{recordCountdown !== null ? `Count-in · ${recordCountdown} beats` : audio.recording.active ? `Recording · ${audio.recording.samplesWritten.toLocaleString()} samples` : audio.message}</div>
      </footer>

      {focusMode && <button className="exit-focus" onClick={() => setFocusMode(false)}>Exit Focus <kbd>Esc</kbd></button>}
      {isMuted && <div className="mute-banner"><Icon name="stop" />EMERGENCY MUTE ENGAGED — audio output is forced silent</div>}
      {commandOpen && <div className="command-backdrop" onMouseDown={() => setCommandOpen(false)}><section className="command-palette" onMouseDown={(event) => event.stopPropagation()}><label><Icon name="command" /><input autoFocus placeholder="Search actions, assets, settings…" /></label><span className="eyebrow">WORKSPACES</span>{workspaces.map((item) => <button key={item.id} onClick={() => { switchWorkspace(item.id); setCommandOpen(false); }}><span>{item.label}</span><small>Switch workspace</small><kbd>{item.key}</kbd></button>)}<footer><span>↑↓ Navigate</span><span>↵ Select</span><span>Esc Close</span></footer></section></div>}
    </main>
  );
}

export default App;
