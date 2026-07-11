import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AudioStatus, BootstrapState, PluginEntry, ScratchSession, Workspace } from "./domain";
import { bootstrap, getAudioStatus, loadPlugin, saveScratch, scanVst3Folder, setEmergencyMute, startRecording, stopRecording } from "./native";

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

function WorkspaceHome({ state, onWorkspace, onQuickRecord }: { state: BootstrapState; onWorkspace: (workspace: Workspace) => void; onQuickRecord: () => void }) {
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
          <button className="quiet" onClick={onQuickRecord}><span className="record-dot" />Quick Record</button>
        </div>
      </section>

      <section className="section-card audio-setup">
        <header><div><span className="eyebrow">AUDIO DEVICE</span><h2>Sound First Setup</h2></div><span className="status-tag warning">ENGINE NEXT</span></header>
        <div className="device-row"><div className="device-icon"><Icon name="bolt" /></div><div><strong>Native audio sidecar</strong><small>WASAPI / ASIO connection is the next delivery gate</small></div><button className="text-button">Configure</button></div>
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

function WorkspacePlay({ session, plugins, setSession }: { session: ScratchSession; plugins: PluginEntry[]; setSession: (value: ScratchSession) => void }) {
  const visiblePlugins = plugins.slice(0, 3);
  return (
    <div className="workspace-scroll play-view">
      <section className="play-header"><div><span className="eyebrow">LIVE SIGNAL</span><h1>Input → Tone → Output</h1></div><div className="snapshot-tabs"><button className="active">A</button><button>B</button><button>＋</button></div></section>
      <div className="signal-line" />
      <section className="rack-flow">
        <article className="rack-device input-device"><span className="device-order">IN</span><div className="device-face"><Meter value={45} /><Meter value={40} /></div><h3>Input 1</h3><small>Mono · −12.4 dB</small></article>
        {(visiblePlugins.length ? visiblePlugins : [{ id: "placeholder", name: "Add a VST3", vendor: null } as PluginEntry]).map((plugin, index) => (
          <article className="rack-device" key={plugin.id}><span className="device-order">{String(index + 1).padStart(2, "0")}</span><div className={`device-face face-${index}`}><span>{plugin.name.slice(0, 2).toUpperCase()}</span><i /></div><h3>{plugin.name}</h3><small>{plugin.vendor ?? "VST3 discovered"}</small><div className="device-controls"><button>Bypass</button><strong>0.0 dB</strong></div></article>
        ))}
        <button className="add-device"><Icon name="plus" /><span>Add Device</span></button>
        <article className="rack-device output-device"><span className="device-order">OUT</span><div className="device-face"><Meter value={58} /><Meter value={51} /></div><h3>Main Out</h3><small>Safety limited</small></article>
      </section>
      <section className="macro-section">
        <header><div><span className="eyebrow">MACROS</span><h2>Performance controls</h2></div><button className="text-button">Map parameters</button></header>
        <div className="macro-grid">
          {["Brightness", "Gain", "Space", "Width"].map((name, i) => <label className="macro" key={name}><span className="knob" style={{ "--turn": `${-120 + i * 45}deg` } as React.CSSProperties}><i /></span><strong>{name}</strong><small>{[42, 61, 28, 76][i]}%</small></label>)}
        </div>
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

function App() {
  const [boot, setBoot] = useState<BootstrapState | null>(null);
  const [session, setSession] = useState<ScratchSession | null>(null);
  const [audio, setAudio] = useState<AudioStatus>({ state: "starting", driver: null, sampleRate: null, bufferSize: null, roundTripMs: null, recording: { active: false, directory: null, sampleRate: null, rawChannels: null, processedChannels: null, samplesWritten: 0, droppedBlocks: 0 }, message: "Audio supervisor is starting." });
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);
  const [scanMessage, setScanMessage] = useState("VST3を検出中…");
  const [librarySection, setLibrarySection] = useState("Plugins");
  const [commandOpen, setCommandOpen] = useState(false);
  const [focusMode, setFocusMode] = useState(false);
  const saveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    void bootstrap().then((state) => {
      setBoot(state);
      setSession(state.session);
      void scanVst3Folder(state.vst3Root).then((report) => {
        setPlugins(report.plugins);
        setScanMessage(report.issues.length ? `${report.plugins.length}件 · ${report.issues.length}件の注意` : `${report.plugins.length}件を検出`);
      });
    });
    const refreshAudio = () => void getAudioStatus().then(setAudio);
    refreshAudio();
    const audioPoll = window.setInterval(refreshAudio, 1000);
    return () => {
      window.clearInterval(audioPoll);
    };
  }, []);

  useEffect(() => {
    if (!session) return;
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => void saveScratch({ ...session, updatedAtMs: Date.now() }), 750);
    return () => window.clearTimeout(saveTimer.current);
  }, [session]);

  const switchWorkspace = useCallback((workspace: Workspace) => {
    setSession((current) => current ? { ...current, workspace } : current);
  }, []);

  const toggleMute = useCallback(async () => {
    if (!session) return;
    const muted = !(session.emergencyMuted || audio.state === "muted");
    setSession({ ...session, emergencyMuted: muted });
    setAudio(await setEmergencyMute(muted));
  }, [audio.state, session]);

  const toggleRecording = useCallback(async () => {
    setAudio(await (audio.recording.active ? stopRecording() : startRecording()));
  }, [audio.recording.active]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA";
      if (event.ctrlKey && event.key.toLowerCase() === "k") { event.preventDefault(); setCommandOpen((open) => !open); return; }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "m") { event.preventDefault(); void toggleMute(); return; }
      if (!typing && event.key >= "1" && event.key <= "6") switchWorkspace(workspaces[Number(event.key) - 1].id);
      if (event.key === "Escape") setCommandOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [switchWorkspace, toggleMute]);

  const selectedPlugin = useMemo(() => plugins[0] ?? null, [plugins]);
  if (!boot || !session) return <div className="boot-screen"><span className="logo-mark">R</span><strong>Riffra</strong><small>Recovering your creative memory…</small></div>;

  const isMuted = session.emergencyMuted || audio.state === "muted";
  return (
    <main className={`app-shell ${focusMode ? "focus-mode" : ""} ${isMuted ? "is-muted" : ""}`}>
      <header className="global-bar">
        <div className="brand"><span className="logo-mark">R</span><strong>RIFFRA</strong></div>
        <button className="session-title"><span className="save-light" />{session.projectName ?? "Untitled Scratch"}<small>Auto-saved</small><Icon name="chevron" /></button>
        <div className="history-controls"><button disabled>↶</button><button disabled>↷</button></div>
        <nav className="workspace-tabs" aria-label="Workspace">
          {workspaces.map((item) => <button key={item.id} className={session.workspace === item.id ? "active" : ""} onClick={() => switchWorkspace(item.id)}>{item.label}<kbd>{item.key}</kbd></button>)}
        </nav>
        <button className="command-trigger" onClick={() => setCommandOpen(true)}><Icon name="search" />Search or command<kbd>Ctrl K</kbd></button>
        <button className={`engine-pill ${audio.state}`}><span />{audio.state === "ready" ? audio.driver : audio.state}<small>{audio.roundTripMs ? `${audio.roundTripMs} ms` : "Audio"}</small></button>
        <button className={`emergency-button ${isMuted ? "active" : ""}`} onClick={() => void toggleMute()}><Icon name="stop" />{isMuted ? "UNMUTE" : "MUTE"}</button>
      </header>

      <aside className="library-panel">
        <div className="panel-heading"><span>LIBRARY</span><button><Icon name="plus" /></button></div>
        <label className="panel-search"><Icon name="search" /><input aria-label="Library search" placeholder="Search assets" /></label>
        <nav>{librarySections.map((section) => <button key={section} className={librarySection === section ? "active" : ""} onClick={() => setLibrarySection(section)}><span className={`nav-glyph glyph-${section.toLowerCase()}`} />{section}<small>{section === "Plugins" ? plugins.length : ""}</small></button>)}</nav>
        <div className="library-content">
          <span className="eyebrow">{librarySection.toUpperCase()}</span>
          {librarySection === "Plugins" ? <><small className="scan-message">{scanMessage}</small>{plugins.slice(0, 12).map((plugin) => <button className="plugin-row" key={plugin.id} onClick={() => void loadPlugin(plugin.path)} title={`Load ${plugin.name}`}><span>{plugin.name.slice(0, 1).toUpperCase()}</span><div><strong>{plugin.name}</strong><small>{plugin.vendor ?? "VST3"}</small></div><i className={`stability ${plugin.scanState}`} /></button>)}</> : <div className="library-empty"><span>まだ資産がありません</span><small>良い結果を保存すると、ここから再利用できます。</small></div>}
        </div>
        <button className="inbox-button"><span className="inbox-icon">↓</span><div><strong>Inbox</strong><small>0 items</small></div></button>
      </aside>

      <section className="workspace">
        {session.workspace === "home" && <WorkspaceHome state={boot} onWorkspace={switchWorkspace} onQuickRecord={() => void toggleRecording()} />}
        {session.workspace === "play" && <WorkspacePlay session={session} plugins={plugins} setSession={setSession} />}
        {!(["home", "play"] as Workspace[]).includes(session.workspace) && <EmptyWorkspace workspace={session.workspace} />}
      </section>

      <aside className="inspector-panel">
        <div className="panel-heading"><span>INSPECTOR</span><button onClick={() => setFocusMode(true)}>×</button></div>
        <div className="inspector-identity"><span className="inspector-art">{selectedPlugin?.name.slice(0, 2).toUpperCase() ?? "SS"}</span><div><span className="eyebrow">{selectedPlugin ? "PLUGIN" : "SESSION"}</span><h3>{selectedPlugin?.name ?? "Scratch Session"}</h3><small>{selectedPlugin?.vendor ?? "Always preserved"}</small></div></div>
        <section><header><strong>Signal</strong><Icon name="chevron" /></header><dl><div><dt>Input</dt><dd>Mono</dd></div><div><dt>Gain</dt><dd>0.0 dB</dd></div><div><dt>State</dt><dd className="safe-label">Safe</dd></div></dl></section>
        <section><header><strong>Provenance</strong><Icon name="chevron" /></header><dl><div><dt>Session</dt><dd>Scratch</dd></div><div><dt>Updated</dt><dd>{new Date(session.updatedAtMs).toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" })}</dd></div></dl></section>
        <section><header><strong>Data safety</strong><Icon name="chevron" /></header><p className="inspector-copy">世代付き自動保存が有効です。現在の作業はプロジェクトへ昇格しなくても保持されます。</p><small className="path-copy">{boot.dataRoot}</small></section>
        <button className="focus-button" onClick={() => setFocusMode(!focusMode)}>{focusMode ? "Exit Focus Mode" : "Focus Mode"}</button>
      </aside>

      <footer className="transport">
        <div className="transport-left"><button><Icon name="loop" /></button><button>◀</button><button className="play-button"><Icon name="play" /></button><button><Icon name="stop" /></button><button className="record-button"><Icon name="record" /></button></div>
        <div className="position"><strong>001 · 01 · 000</strong><small>00:00:00.000</small></div>
        <div className="tempo"><button><strong>120.00</strong><small>BPM</small></button><button><strong>4 / 4</strong><small>TIME</small></button></div>
        <div className="transport-meter"><span>IN</span><Meter value={42} /><span>OUT</span><Meter value={56} /></div>
        <div className="master"><span>MASTER</span><strong>{session.masterDb.toFixed(1)} dB</strong><input aria-label="Master volume" type="range" min="-60" max="0" step="0.5" value={session.masterDb} onChange={(event) => setSession({ ...session, masterDb: Number(event.target.value) })} /></div>
        <div className="status-line"><span className={`status-dot ${audio.state}`} />{audio.message}</div>
      </footer>

      {focusMode && <button className="exit-focus" onClick={() => setFocusMode(false)}>Exit Focus <kbd>Esc</kbd></button>}
      {isMuted && <div className="mute-banner"><Icon name="stop" />EMERGENCY MUTE ENGAGED — audio output is forced silent</div>}
      {commandOpen && <div className="command-backdrop" onMouseDown={() => setCommandOpen(false)}><section className="command-palette" onMouseDown={(event) => event.stopPropagation()}><label><Icon name="command" /><input autoFocus placeholder="Search actions, assets, settings…" /></label><span className="eyebrow">WORKSPACES</span>{workspaces.map((item) => <button key={item.id} onClick={() => { switchWorkspace(item.id); setCommandOpen(false); }}><span>{item.label}</span><small>Switch workspace</small><kbd>{item.key}</kbd></button>)}<footer><span>↑↓ Navigate</span><span>↵ Select</span><span>Esc Close</span></footer></section></div>}
    </main>
  );
}

export default App;
