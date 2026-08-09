import type { NativeApi } from '@/native/native-api';
import type { PluginEntry } from '@/lib/domain';
import { useEffect, useState } from 'react';
import { logNativeError } from '@/native/invoke';
import { defaultNativeApi } from '@/native/native';
import { useApp } from '@/hooks/useApp';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';
import { workspaces } from '@/constants';
import { isOutputMuted } from '@/lib/audio-safety';
import { useAudioFeedbackSuspected } from '@/lib/audio-meters';
import {
  AudioSettingsDialog,
  Icon,
  WorkspaceAnalyze,
  WorkspaceSample,
  MidiDevices,
  MidiMonitor,
  SamplePadEditor,
  SamplePreviewControls,
  ReferenceSuggestion,
  WorkspaceSeparate,
  GlobalBar,
  LibraryPanel,
  InspectorPanel,
  TransportBar,
  MissingDependencies,
  WorkspaceArrange,
} from '@/components';
import styles from './App.module.css';

export default function App({ api = defaultNativeApi }: { api?: NativeApi } = {}) {
  const [arrangeSelection, setArrangeSelection] = useState<ArrangeSelection>({ kind: 'none' });
  const [arrangeFocusedTrackId, setArrangeFocusedTrackId] = useState<string | null>(null);
  const [audioSettingsOpen, setAudioSettingsOpen] = useState(false);
  const {
    boot,
    session,
    audio,
    setAudio,
    focusMode,
    libraryQuery,
    librarySection,
    importMidi,
    libraryResults,
    plugins,
    visiblePlugins,
    visibleRecordings,
    usableRecordings,
    inbox,
    selectedLibraryAsset,
    relatedAssets,
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
    separationPreviewingAssetId,
    previewPadId,
    transportPlaying,
    recordingCommandPending,
    autosaveError,
    audioPreferenceMessage,
    exportMessage,
    deviceProbe,
    midi,
    missingDependencies,
    backgroundJob,
    cancelActiveJob,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
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
    openRecordingAnalysis,
    openLibraryAssetAnalysis,
    recoverAudio,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
    selectAudioDriver,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
    previewSamplePad,
    stopPreview,
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
    goToStart,
    toggleRecording,
    api: nativeApi,
  } = useApp(api);
  const liveFeedbackSuspected = useAudioFeedbackSuspected();
  const focusedTrack =
    session?.arrangement.tracks.find((track) => track.id === arrangeFocusedTrackId) ?? null;

  useEffect(() => {
    if (
      arrangeFocusedTrackId !== null &&
      !session?.arrangement.tracks.some((track) => track.id === arrangeFocusedTrackId)
    ) {
      setArrangeFocusedTrackId(null);
    }
  }, [arrangeFocusedTrackId, session?.arrangement.tracks]);

  const addPluginToFocusedTrack = async (plugin: PluginEntry, target: 'instrument' | 'effect') => {
    if (!focusedTrack) return;
    try {
      const next =
        target === 'instrument'
          ? await nativeApi.setTrackInstrument(focusedTrack.id, plugin.path)
          : await nativeApi.addTrackEffect(focusedTrack.id, plugin.path);
      setSession(next);
    } catch (error) {
      logNativeError('Add plugin to Track')(error);
    }
  };
  const refreshAudioDevices = async () => {
    const nextProbe = await nativeApi.probeAudioDevices();
    setDeviceProbe(nextProbe);
    return nextProbe;
  };
  const probeAudioChannels = async (driver: string, inputDevice: string, outputDevice: string) =>
    api.probeDeviceChannels(driver, inputDevice, outputDevice);
  if (!boot || !session)
    return (
      <div className={styles.bootScreen}>
        <span className="logo-mark">R</span>
        <strong>Riffra</strong>
        <small>Recovering your creative memory…</small>
      </div>
    );

  const feedbackSuspected = liveFeedbackSuspected || audio.feedbackSuspected;
  const isMuted = isOutputMuted(audio) || feedbackSuspected;
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
        onOpenAudioSettings={() => setAudioSettingsOpen(true)}
        audioSettingsOpen={audioSettingsOpen}
      />

      <AudioSettingsDialog
        open={audioSettingsOpen}
        audio={audio}
        probe={deviceProbe}
        safeMode={boot.safeMode}
        recordingActive={audio.recording.active || recordingCommandPending}
        onClose={() => setAudioSettingsOpen(false)}
        onRefresh={refreshAudioDevices}
        onProbeChannels={probeAudioChannels}
        onApply={selectAudioDriver}
        onRecover={recoverAudio}
      />

      {boot.safeMode && (
        <div className={`${styles.shellNotice} ${styles.safeModeNotice}`} role="status">
          <strong>SAFE MODE</strong>
          <span>
            External VST3, MIDI input, driver changes, live preview and new recordings are isolated.
            Project open, library access, offline analysis, render and export remain available.
            Restart without <code>--safe-mode</code> to reconnect devices.
          </span>
        </div>
      )}

      {boot.recoveredFromGeneration && boot.recoveryCandidates.length > 0 && (
        <div className={`${styles.shellNotice} ${styles.recoveryNotice}`} role="status">
          <strong>RECOVERY CHOICE</strong>
          <span>
            The current session was recovered from an autosave generation. Choose a previous stable
            generation if needed.
          </span>
          <div className={styles.recoveryActions}>
            {boot.recoveryCandidates.slice(0, 5).map((candidate) => (
              <button
                className="text-button"
                key={candidate.fileName}
                onClick={() => void restoreRecovery(candidate.fileName)}
              >
                {candidate.projectName ?? 'Untitled'} ·{' '}
                {new Date(candidate.updatedAtMs).toLocaleString('ja-JP')}
              </button>
            ))}
            <button className="text-button" onClick={dismissRecovery}>
              Keep recovered session
            </button>
          </div>
        </div>
      )}

      {!boot.nativeAvailable && (
        <div className={styles.runtimeBanner}>
          <strong>BROWSER PREVIEW</strong>
          <span>
            Native audio, VST3, MIDI, recording and Windows persistence are unavailable here. Open
            the Tauri application to use product features; this preview does not report empty
            results as successful operations.
          </span>
        </div>
      )}

      {backgroundJob && (
        <div className={styles.runtimeBanner}>
          <strong>
            {backgroundJob.kind.toUpperCase()} JOB · {backgroundJob.state.toUpperCase()}
          </strong>
          <span>{backgroundJob.message}</span>
          {['queued', 'running', 'cancelling'].includes(backgroundJob.state) && (
            <button className="text-button" onClick={() => void cancelActiveJob()}>
              Cancel
            </button>
          )}
        </div>
      )}

      {missingDependencies.length > 0 && (
        <MissingDependencies
          missing={missingDependencies}
          onRelink={relinkMissing}
          onReplacePlugin={(deviceId, newPath) =>
            void replaceMissingPluginDevice(deviceId, newPath)
          }
          onDisablePlugin={disableMissingPluginDevice}
          onIgnore={ignoreMissing}
        />
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
          onOpenInDesign: openLibraryAssetAnalysis,
          onImportMidi: () => void importMidi(),
        }}
        rack={{
          plugins,
          visiblePlugins,
          focusedTrack,
          onAddPlugin: (plugin, target) => void addPluginToFocusedTrack(plugin, target),
        }}
        recordings={{
          visibleRecordings,
          count: recordings.length,
          onOpenRecording: openRecordingAnalysis,
        }}
        inbox={inbox}
      />

      <section className="workspace">
        {session.workspace === 'arrange' && (
          <WorkspaceArrange
            session={session}
            setSession={setSession}
            selection={arrangeSelection}
            setSelection={setArrangeSelection}
            api={nativeApi}
            audio={audio}
            plugins={plugins}
            focusedTrackId={arrangeFocusedTrackId}
            onFocusTrack={setArrangeFocusedTrackId}
            missingDeviceIds={missingDependencies
              .filter((item) => item.kind === 'plugin')
              .map((item) => item.id)}
            onRecord={() => void toggleRecording()}
            recordingActive={audio.recording.active}
          />
        )}
        {session.workspace === 'design' && session.designContext.activeTool === 'sample' && (
          <>
            <WorkspaceSample
              session={session}
              recordings={usableRecordings}
              onCreateSamplePad={createSamplePad}
              onPreviewPad={(pad) => void previewSamplePad(pad)}
            />
            <SamplePadEditor
              session={session}
              updateSamplePad={updateSamplePad}
              removeSamplePad={removeSamplePad}
            />
            <SamplePreviewControls
              session={session}
              playingId={previewPadId}
              onPreview={(pad) => void previewSamplePad(pad)}
              onStop={() => void stopPreview()}
            />
            <MidiDevices
              probe={midi}
              onRefresh={() =>
                void nativeApi
                  .probeMidiDevices()
                  .then(setMidi)
                  .catch(logNativeError('probeMidiDevices'))
              }
            />
            <MidiMonitor probe={midi} audio={audio} onPanic={() => void stopPreview()} />
          </>
        )}
        {session.workspace === 'design' && session.designContext.activeTool === 'analyze' && (
          <>
            <WorkspaceAnalyze analysis={analysis} />
            <ReferenceSuggestion
              analysis={analysis}
              recordings={usableRecordings}
              references={referenceAnalyses}
              referenceId={referenceId}
              session={session}
              setSession={setSession}
              api={nativeApi}
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
        {session.workspace === 'design' && session.designContext.activeTool === 'separate' && (
          <WorkspaceSeparate
            recordings={usableRecordings}
            results={separations}
            busyId={separationBusy}
            message={separationMessage}
            previewingAssetId={separationPreviewingAssetId}
            onSeparate={(recording) => void runSeparation(recording)}
            onPreview={(assetId) => void previewSeparation(assetId)}
            onStop={() => void stopSeparationPreview()}
            onAddToTimeline={(assetId, name, durationMs) =>
              void addSeparationToTimeline(assetId, name, durationMs)
            }
          />
        )}
      </section>

      <InspectorPanel
        audio={audio}
        boot={boot}
        focusMode={focusMode}
        setFocusMode={setFocusMode}
        session={session}
        setSession={setSession}
        arrangeSelection={arrangeSelection}
        setArrangeSelection={setArrangeSelection}
        missingDependencies={missingDependencies}
        plugins={plugins}
        onDisableMissingPlugin={disableMissingPluginDevice}
        onReplaceMissingPlugin={replaceMissingPluginDevice}
        onRescanMissingPlugins={rescanMissingPlugins}
        api={nativeApi}
      />

      <TransportBar
        session={session}
        setSession={setSession}
        audio={audio}
        setAudio={setAudio}
        transportPlaying={transportPlaying}
        onPlay={playTransport}
        onStop={stopTransport}
        onGoToStart={goToStart}
        recordingCommandPending={recordingCommandPending}
        onToggleRecording={toggleRecording}
        autosaveError={autosaveError}
        audioPreferenceMessage={audioPreferenceMessage}
        projectActionMessage={exportMessage}
        api={nativeApi}
      />

      {focusMode && (
        <button className={styles.exitFocus} onClick={() => setFocusMode(false)}>
          Exit Focus <kbd>Esc</kbd>
        </button>
      )}
      {isMuted && (
        <div className={styles.muteBanner}>
          <Icon name="stop" />
          EMERGENCY MUTE ENGAGED —{' '}
          {feedbackSuspected
            ? 'acoustic feedback suspected; output silenced automatically'
            : 'audio output is forced silent'}
        </div>
      )}
      {commandOpen && (
        <div className={styles.commandBackdrop} onMouseDown={() => setCommandOpen(false)}>
          <section
            className={styles.commandPalette}
            onMouseDown={(event) => event.stopPropagation()}
          >
            <label>
              <Icon name="command" />
              <input autoFocus placeholder="Search actions, assets, settings…" />
            </label>
            <span className="eyebrow">WORKSPACES</span>
            {workspaces.map((item) => (
              <button
                key={item.id}
                onClick={() => {
                  void switchWorkspace(item.id);
                  setCommandOpen(false);
                }}
              >
                <span>{item.label}</span>
                <small>Switch workspace</small>
                <kbd>{item.key}</kbd>
              </button>
            ))}
            <span className="eyebrow">PROJECT</span>
            <button
              onClick={() => {
                setCommandOpen(false);
                void importSession();
              }}
            >
              <span>Import Project</span>
              <small>Open a project.json manifest</small>
            </button>
            <button
              onClick={() => {
                setCommandOpen(false);
                void exportSession();
              }}
            >
              <span>Export Project</span>
              <small>Write a collected project manifest</small>
            </button>
            <span className="eyebrow">SETTINGS</span>
            <button
              onClick={() => {
                setCommandOpen(false);
                setAudioSettingsOpen(true);
              }}
            >
              <span>Audio Settings</span>
              <small>Configure driver and Windows devices</small>
            </button>
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
