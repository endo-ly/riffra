import type { Workspace } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { defaultNativeApi } from '@/native/native';
import { useApp } from '@/hooks/useApp';
import { workspaces } from '@/constants';
import { isOutputMuted } from '@/lib/audio-safety';
import {
  AudioDriverPicker,
  CaptureSettings,
  Icon,
  WorkspaceHome,
  AudioDevices,
  WorkspacePlay,
  EmptyWorkspace,
  WorkspaceAnalyze,
  WorkspaceArrange,
  WorkspaceSample,
  MidiDevices,
  MidiMonitor,
  SamplePadEditor,
  SamplePreviewControls,
  TimelineClipInspector,
  TimelineRenderControls,
  MidiClipEditor,
  ReferenceSuggestion,
  WorkspaceSeparate,
  GlobalBar,
  LibraryPanel,
  InspectorPanel,
  TransportBar,
} from '@/components';

export default function App({ api = defaultNativeApi }: { api?: NativeApi } = {}) {
  const {
    boot,
    session,
    audio,
    setAudio,
    focusMode,
    libraryQuery,
    librarySection,
    libraryResults,
    plugins,
    visiblePlugins,
    visibleRecordings,
    usableRecordings,
    selectedLibraryAsset,
    relatedAssets,
    selectedPluginName,
    selectedPluginVendor,
    query,
    recordings,
    analysis,
    referenceAnalyses,
    referenceId,
    referencePreviewingId,
    referenceSyncPreviewing,
    referenceLoopPreview,
    separations,
    separationBusy,
    separationMessage,
    separationPreviewingPath,
    renderResult,
    stemResults,
    renderMessage,
    renderPreviewing,
    previewPadId,
    transportPlaying,
    recordingCommandPending,
    recordCountdown,
    autosaveError,
    audioPreferenceMessage,
    exportMessage,
    deviceProbe,
    midi,
    missingPluginPaths,
    commandOpen,
    undoStack,
    redoStack,
    setSession,
    setLibraryQuery,
    setLibrarySection,
    setCommandOpen,
    setFocusMode,
    setReferenceLoopPreview,
    setDeviceProbe,
    setMidi,
    renameSession,
    undo,
    redo,
    switchWorkspace,
    toggleMute,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    editSelectedLibraryAsset,
    loadPluginIntoRack,
    openRecordingAnalysis,
    recoverAudio,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
    selectAudioDriver,
    togglePluginBypass,
    setPluginParameterValue,
    clearPluginFromRack,
    captureSnapshot,
    recallSnapshot,
    placeRecording,
    runTimelineRender,
    runTimelineStemRender,
    previewTimelineRender,
    stopTimelinePreview,
    createSamplePad,
    previewSamplePad,
    stopPreview,
    connectMidiInput,
    disconnectMidiInput,
    selectReference,
    previewReference,
    stopReferencePreview,
    previewReferencePair,
    addSeparationToTimeline,
    runSeparation,
    previewSeparation,
    stopSeparationPreview,
    playTransport,
    stopTransport,
    toggleRecording,
    api: nativeApi,
  } = useApp(api);
  if (!boot || !session)
    return (
      <div className="boot-screen">
        <span className="logo-mark">R</span>
        <strong>Riffra</strong>
        <small>Recovering your creative memory…</small>
      </div>
    );

  const isMuted = isOutputMuted(session.emergencyMuted, audio);
  return (
    <main className={`app-shell ${focusMode ? 'focus-mode' : ''} ${isMuted ? 'is-muted' : ''}`}>
      <GlobalBar
        session={session}
        audio={audio}
        isMuted={isMuted}
        undoStack={undoStack}
        redoStack={redoStack}
        onUndo={undo}
        onRedo={redo}
        onSwitchWorkspace={switchWorkspace}
        onRenameSession={renameSession}
        onToggleMute={toggleMute}
        onOpenCommand={() => setCommandOpen(true)}
      />

      {!boot.nativeAvailable && (
        <div className="runtime-banner">
          <strong>BROWSER PREVIEW</strong>
          <span>
            Native audio, VST3, MIDI, recording and Windows persistence are unavailable here. Open
            the Tauri application to use product features; this preview does not report empty
            results as successful operations.
          </span>
        </div>
      )}

      <LibraryPanel
        library={{
          section: librarySection,
          setSection: setLibrarySection,
          query: libraryQuery,
          setQuery: setLibraryQuery,
          results: libraryResults,
          searchQuery: query,
          selectedAsset: selectedLibraryAsset,
          relatedAssets,
          onSelectAsset: selectLibraryAsset,
          onPreviewAsset: previewSelectedLibraryAsset,
          onEditAsset: editSelectedLibraryAsset,
        }}
        rack={{
          plugins,
          visiblePlugins,
          onLoadPlugin: loadPluginIntoRack,
        }}
        recordings={{
          visibleRecordings,
          count: recordings.length,
          onOpenRecording: openRecordingAnalysis,
        }}
      />

      <section className="workspace">
        {session.workspace === 'home' && (
          <div className="workspace-scroll home-grid">
            <WorkspaceHome
              state={boot}
              onWorkspace={switchWorkspace}
              onQuickRecord={() => void toggleRecording()}
              recordingActive={
                audio.recording.active || recordCountdown !== null || recordingCommandPending
              }
              onRecoverAudioDevice={() => void recoverAudio()}
              onExportProject={() => void exportSession()}
              onImportProject={() => void importSession()}
              onRestoreRecovery={(fileName) => void restoreRecovery(fileName)}
              onDismissRecovery={dismissRecovery}
              exportMessage={exportMessage}
            />
            <AudioDevices
              probe={deviceProbe}
              onRefresh={() => void nativeApi.probeAudioDevices().then(setDeviceProbe)}
            />
            <AudioDriverPicker
              probe={deviceProbe}
              current={audio.driver}
              sampleRate={audio.sampleRate}
              bufferSize={audio.bufferSize}
              onSelect={(driver, sampleRate, bufferSize) =>
                void selectAudioDriver(driver, sampleRate, bufferSize)
              }
            />
            <CaptureSettings session={session} setSession={setSession} />
          </div>
        )}
        {session.workspace === 'play' && (
          <WorkspacePlay
            session={session}
            audio={audio}
            plugins={plugins}
            missingPluginPaths={missingPluginPaths}
            setSession={setSession}
            onTogglePluginBypass={(bypassed) => void togglePluginBypass(bypassed)}
            onSetPluginParameter={(index, value) => void setPluginParameterValue(index, value)}
            onClearPlugin={() => void clearPluginFromRack()}
            onCaptureSnapshot={captureSnapshot}
            onRecallSnapshot={(slot) => void recallSnapshot(slot)}
          />
        )}
        {session.workspace === 'arrange' && (
          <>
            <WorkspaceArrange
              session={session}
              setSession={setSession}
              recordings={usableRecordings}
              onPlaceRecording={placeRecording}
            />
            <TimelineClipInspector session={session} setSession={setSession} />
            <MidiClipEditor
              session={session}
              setSession={setSession}
              recordings={usableRecordings}
              api={nativeApi}
            />
            <TimelineRenderControls
              session={session}
              result={renderResult}
              stems={stemResults}
              message={renderMessage}
              onRender={(options) => void runTimelineRender(options)}
              onRenderStems={(options) => void runTimelineStemRender(options)}
              onPreview={() => void previewTimelineRender()}
              onStop={() => void stopTimelinePreview()}
              previewing={renderPreviewing}
            />
          </>
        )}
        {session.workspace === 'sample' && (
          <>
            <WorkspaceSample
              session={session}
              recordings={usableRecordings}
              onCreateSamplePad={createSamplePad}
              onPreviewPad={(pad) => void previewSamplePad(pad)}
            />
            <SamplePadEditor session={session} setSession={setSession} />
            <SamplePreviewControls
              session={session}
              playingId={previewPadId}
              onPreview={(pad) => void previewSamplePad(pad)}
              onStop={() => void stopPreview()}
            />
            <MidiDevices
              probe={midi}
              onRefresh={() => void nativeApi.probeMidiDevices().then(setMidi)}
            />
            <MidiMonitor
              probe={midi}
              audio={audio}
              onOpen={(name) => void connectMidiInput(name)}
              onClose={() => void disconnectMidiInput()}
              onPanic={() => void stopPreview()}
            />
          </>
        )}
        {session.workspace === 'analyze' && (
          <>
            <WorkspaceAnalyze analysis={analysis} />
            <ReferenceSuggestion
              analysis={analysis}
              recordings={usableRecordings}
              references={referenceAnalyses}
              referenceId={referenceId}
              session={session}
              setSession={setSession}
              onSelect={(recording) => void selectReference(recording)}
              onPreview={(recording) => void previewReference(recording)}
              onStop={() => void stopReferencePreview()}
              onSyncPreview={() => void previewReferencePair()}
              onToggleLoop={() => setReferenceLoopPreview((value) => !value)}
              previewingId={referencePreviewingId}
              syncPreviewing={referenceSyncPreviewing}
              loopPreview={referenceLoopPreview}
            />
          </>
        )}
        {session.workspace === 'separate' && (
          <WorkspaceSeparate
            recordings={usableRecordings}
            results={separations}
            busyId={separationBusy}
            message={separationMessage}
            previewingPath={separationPreviewingPath}
            onSeparate={(recording) => void runSeparation(recording)}
            onPreview={(path) => void previewSeparation(path)}
            onStop={() => void stopSeparationPreview()}
            onAddToTimeline={(path, name) => void addSeparationToTimeline(path, name)}
          />
        )}
        {!(['home', 'play', 'arrange', 'sample', 'analyze', 'separate'] as Workspace[]).includes(
          session.workspace,
        ) && <EmptyWorkspace workspace={session.workspace} />}
      </section>

      <InspectorPanel
        session={session}
        audio={audio}
        boot={boot}
        focusMode={focusMode}
        setFocusMode={setFocusMode}
        selectedPluginName={selectedPluginName}
        selectedPluginVendor={selectedPluginVendor}
      />

      <TransportBar
        session={session}
        setSession={setSession}
        audio={audio}
        setAudio={setAudio}
        transportPlaying={transportPlaying}
        onPlay={playTransport}
        onStop={stopTransport}
        recordingCommandPending={recordingCommandPending}
        onToggleRecording={toggleRecording}
        recordCountdown={recordCountdown}
        autosaveError={autosaveError}
        audioPreferenceMessage={audioPreferenceMessage}
        api={nativeApi}
      />

      {focusMode && (
        <button className="exit-focus" onClick={() => setFocusMode(false)}>
          Exit Focus <kbd>Esc</kbd>
        </button>
      )}
      {isMuted && (
        <div className="mute-banner">
          <Icon name="stop" />
          EMERGENCY MUTE ENGAGED — audio output is forced silent
        </div>
      )}
      {commandOpen && (
        <div className="command-backdrop" onMouseDown={() => setCommandOpen(false)}>
          <section className="command-palette" onMouseDown={(event) => event.stopPropagation()}>
            <label>
              <Icon name="command" />
              <input autoFocus placeholder="Search actions, assets, settings…" />
            </label>
            <span className="eyebrow">WORKSPACES</span>
            {workspaces.map((item) => (
              <button
                key={item.id}
                onClick={() => {
                  switchWorkspace(item.id);
                  setCommandOpen(false);
                }}
              >
                <span>{item.label}</span>
                <small>Switch workspace</small>
                <kbd>{item.key}</kbd>
              </button>
            ))}
            <footer>
              <span>↑↓ Navigate</span>
              <span>↵ Select</span>
              <span>Esc Close</span>
            </footer>
          </section>
        </div>
      )}
    </main>
  );
}
