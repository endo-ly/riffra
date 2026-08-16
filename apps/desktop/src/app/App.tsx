import type { NativeApi } from '@/native/native-api';
import clsx from 'clsx';
import {
  useEffect,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import { defaultNativeApi } from '@/native/native';
import { useAppController } from '@/app/useAppController';
import { useArrangeShell } from '@/features/arrange/hooks/useArrangeShell';
import { WorkspaceArrange } from '@/features/arrange/WorkspaceArrange';
import { InspectorPanel } from '@/features/arrange/inspector/InspectorPanel';
import { LibraryPanel } from '@/features/library/LibraryPanel';
import { MissingDependencies } from '@/features/project/MissingDependencies';
import { AudioSettingsDialog } from '@/features/audio/AudioSettingsDialog';
import { Icon } from '@/shared/ui/primitives';
import surface from '@/shared/ui/Surface.module.css';
import { ToastStack } from '@/shared/ui/ToastStack';
import { GlobalControlBar } from './layout/GlobalControlBar';
import { NavigationRail } from './layout/NavigationRail';
import { SidePanel, type SidePanelView } from './layout/SidePanel';
import { isEmergencyMuteActive } from '@/shared/audio/audio-safety';
import { useAudioFeedbackSuspected } from '@/shared/audio/audio-meters';
import { clearToast, showToast, toast } from '@/shared/toasts';
import styles from './App.module.css';
import shellStyles from './AppShell.module.css';

const SIDE_PANEL_WIDTH = { default: 280, min: 220, max: 380, collapse: 48 } as const;

function resolveSidePanelWidth(width: number) {
  if (width <= SIDE_PANEL_WIDTH.collapse) return 0;
  return Math.min(SIDE_PANEL_WIDTH.max, Math.max(SIDE_PANEL_WIDTH.min, width));
}

function adjustSidePanelWidth(width: number, delta: number) {
  if (width === 0 && delta > 0) return SIDE_PANEL_WIDTH.min;
  return resolveSidePanelWidth(width + delta);
}

function PanelResizeHandle(props: {
  width: number;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onResizeBy: (delta: number) => void;
}) {
  return (
    <div
      className={clsx(shellStyles.panelResizeHandle, props.width === 0 && shellStyles.collapsed)}
      role="separator"
      aria-label="Resize or collapse side panel"
      aria-orientation="vertical"
      aria-valuemin={0}
      aria-valuemax={SIDE_PANEL_WIDTH.max}
      aria-valuenow={props.width}
      tabIndex={0}
      onPointerDown={props.onPointerDown}
      onKeyDown={(event) => {
        if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
          event.preventDefault();
          props.onResizeBy((event.key === 'ArrowRight' ? 1 : -1) * (event.shiftKey ? 24 : 8));
        }
      }}
    />
  );
}

export default function App({ api = defaultNativeApi }: { api?: NativeApi } = {}) {
  const [sidePanelView, setSidePanelView] = useState<SidePanelView>('browser');
  const [sidePanelOpen, setSidePanelOpen] = useState(true);
  const [sidePanelWidth, setSidePanelWidth] = useState<number>(SIDE_PANEL_WIDTH.default);
  const [performanceHost, setPerformanceHost] = useState<HTMLDivElement | null>(null);
  const [panelResize, setPanelResize] = useState<{
    startX: number;
    startWidth: number;
  } | null>(null);
  const [audioSettingsOpen, setAudioSettingsOpen] = useState(false);
  const {
    boot,
    session,
    audio,
    setAudio,
    libraryQuery,
    librarySection,
    importMidi,
    libraryResults,
    plugins,
    visiblePlugins,
    visibleRecordings,
    inbox,
    selectedLibraryAsset,
    relatedAssets,
    query,
    recordings,
    transportPlaying,
    recordingCommandPending,
    startRecordingNow,
    autosaveError,
    audioPreferenceMessage,
    exportMessage,
    deviceProbe,
    missingDependencies,
    backgroundJob,
    cancelActiveJob,
    relinkMissing,
    disableMissingPluginDevice,
    replaceMissingPluginDevice,
    rescanMissingPlugins,
    ignoreMissing,
    commandOpen,
    historyState,
    setSession,
    setLibraryQuery,
    setLibrarySection,
    setCommandOpen,
    refreshAudioDevices,
    probeAudioChannels,
    renameSession,
    undo,
    redo,
    toggleMute,
    selectLibraryAsset,
    previewSelectedLibraryAsset,
    editSelectedLibraryAsset,
    recoverAudio,
    exportSession,
    importSession,
    restoreRecovery,
    dismissRecovery,
    selectAudioDriver,
    stopTransport,
    playTransport,
    goToStart,
    toggleRecording,
    api: nativeApi,
  } = useAppController(api);
  const arrange = useArrangeShell(nativeApi, session, setSession);
  const liveFeedbackSuspected = useAudioFeedbackSuspected();

  useEffect(() => {
    if (!panelResize) return;
    const onPointerMove = (event: PointerEvent) => {
      const delta = event.clientX - panelResize.startX;
      const nextWidth = adjustSidePanelWidth(panelResize.startWidth, delta);
      setSidePanelWidth(nextWidth);
      setSidePanelOpen(nextWidth > 0);
    };
    const stopResize = () => setPanelResize(null);
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', stopResize);
    window.addEventListener('pointercancel', stopResize);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', stopResize);
      window.removeEventListener('pointercancel', stopResize);
    };
  }, [panelResize]);

  const startPanelResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    setPanelResize({
      startX: event.clientX,
      startWidth: sidePanelOpen ? sidePanelWidth : 0,
    });
  };

  const resizeSidePanelBy = (delta: number) => {
    const nextWidth = adjustSidePanelWidth(sidePanelOpen ? sidePanelWidth : 0, delta);
    setSidePanelWidth(nextWidth);
    setSidePanelOpen(nextWidth > 0);
  };

  const selectSidePanel = (view: SidePanelView) => {
    if (sidePanelOpen && sidePanelView === view) {
      setSidePanelOpen(false);
      return;
    }
    setSidePanelView(view);
    setSidePanelOpen(true);
    setSidePanelWidth((width) => Math.max(SIDE_PANEL_WIDTH.min, width));
  };

  useEffect(() => {
    if (!autosaveError) {
      clearToast('app.autosave-error');
      return;
    }
    showToast('app.autosave-error', autosaveError, { kind: 'error', persistent: true });
    return () => clearToast('app.autosave-error');
  }, [autosaveError]);
  useEffect(() => {
    if (!audioPreferenceMessage) {
      clearToast('app.audio-preference');
      return;
    }
    showToast('app.audio-preference', audioPreferenceMessage, { kind: 'info', persistent: true });
    return () => clearToast('app.audio-preference');
  }, [audioPreferenceMessage]);
  useEffect(() => {
    if (exportMessage) toast(exportMessage, { kind: 'info' });
  }, [exportMessage]);

  if (!boot || !session)
    return (
      <div className={styles.bootScreen}>
        <span className={shellStyles.logoMark}>R</span>
        <strong>Riffra</strong>
        <small>Recovering your creative memory…</small>
      </div>
    );

  const feedbackSuspected = liveFeedbackSuspected || audio.feedbackSuspected;
  const isMuted = isEmergencyMuteActive(audio);
  const shellStyle = {
    '--side-panel-width': `${sidePanelOpen ? sidePanelWidth : 0}px`,
  } as CSSProperties;
  return (
    <main
      className={clsx(shellStyles.appShell, panelResize && shellStyles.isPanelResizing)}
      style={shellStyle}
    >
      <GlobalControlBar
        session={session}
        audio={audio}
        isMuted={isMuted}
        historyState={historyState}
        onUndo={() => void undo()}
        onRedo={() => void redo()}
        onRenameSession={() => void renameSession()}
        onToggleMute={() => void toggleMute()}
        onOpenCommand={() => setCommandOpen(true)}
        onOpenAudioSettings={() => setAudioSettingsOpen(true)}
        audioSettingsOpen={audioSettingsOpen}
        setSession={setSession}
        setAudio={setAudio}
        transportPlaying={transportPlaying}
        onPlay={() => void playTransport()}
        onStop={() => void stopTransport()}
        onGoToStart={() => void goToStart()}
        recordingCommandPending={recordingCommandPending}
        onToggleRecording={() => void toggleRecording()}
        transportApi={nativeApi}
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
                className={surface.textButton}
                key={candidate.fileName}
                onClick={() => void restoreRecovery(candidate.fileName)}
              >
                {candidate.projectName ?? 'Untitled'} ·{' '}
                {new Date(candidate.updatedAtMs).toLocaleString('ja-JP')}
              </button>
            ))}
            <button className={surface.textButton} onClick={dismissRecovery}>
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
            <button className={surface.textButton} onClick={() => void cancelActiveJob()}>
              Cancel
            </button>
          )}
        </div>
      )}

      {missingDependencies.length > 0 && (
        <MissingDependencies
          missing={missingDependencies}
          onRelink={(item, newPath) => void relinkMissing(item, newPath)}
          onReplacePlugin={(deviceId, newPath) =>
            void replaceMissingPluginDevice(deviceId, newPath)
          }
          onDisablePlugin={(deviceId) => void disableMissingPluginDevice(deviceId)}
          onIgnore={ignoreMissing}
        />
      )}

      <div className={shellStyles.appBody}>
        <NavigationRail
          activeView={sidePanelView}
          open={sidePanelOpen}
          onSelect={selectSidePanel}
        />

        {sidePanelOpen && (
          <SidePanel view={sidePanelView}>
            {sidePanelView === 'browser' ? (
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
                  onSelectAsset: (asset) => void selectLibraryAsset(asset),
                  onPreviewAsset: () => void previewSelectedLibraryAsset(),
                  onEditAsset: () => void editSelectedLibraryAsset(),
                  onImportMidi: () => void importMidi(),
                }}
                plugins={{
                  plugins,
                  visiblePlugins,
                  selectedTrack: arrange.selectedTrack,
                  onAddPlugin: (plugin, target) => void arrange.addPlugin(plugin, target),
                }}
                recordings={{
                  visibleRecordings,
                  count: recordings.length,
                }}
                inbox={inbox}
              />
            ) : (
              <InspectorPanel
                audio={audio}
                recordingCommandPending={recordingCommandPending}
                session={session}
                setSession={setSession}
                arrangeSelection={arrange.selection}
                setArrangeSelection={arrange.setSelection}
                missingDependencies={missingDependencies}
                plugins={plugins}
                onDisableMissingPlugin={disableMissingPluginDevice}
                onReplaceMissingPlugin={replaceMissingPluginDevice}
                onRescanMissingPlugins={rescanMissingPlugins}
                onRecordAnotherTake={(recordingSessionId) =>
                  void startRecordingNow(recordingSessionId)
                }
                api={nativeApi}
              />
            )}
          </SidePanel>
        )}

        <PanelResizeHandle
          width={sidePanelOpen ? sidePanelWidth : 0}
          onPointerDown={startPanelResize}
          onResizeBy={resizeSidePanelBy}
        />

        <section className={shellStyles.workspace}>
          <WorkspaceArrange
            session={session}
            setSession={setSession}
            selection={arrange.selection}
            setSelection={arrange.setSelection}
            api={nativeApi}
            audio={audio}
            onToggleTransport={() => void (transportPlaying ? stopTransport() : playTransport())}
            plugins={plugins}
            focusedTrackId={arrange.focusedTrackId}
            onFocusTrack={arrange.setFocusedTrackId}
            canonicalOperationPending={arrange.canonicalOperationPending}
            missingDeviceIds={missingDependencies
              .filter((item) => item.kind === 'plugin')
              .map((item) => item.id)}
            performanceHost={performanceHost}
          />
        </section>
      </div>

      <div ref={setPerformanceHost} className={shellStyles.performanceHost} data-performance-host />

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
            <span className={clsx(surface.eyebrow, styles.commandEyebrow)}>PROJECT</span>
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
            <span className={clsx(surface.eyebrow, styles.commandEyebrow)}>SETTINGS</span>
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

      <ToastStack />
    </main>
  );
}
