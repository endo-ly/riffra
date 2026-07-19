import type { BootstrapState, Workspace } from '@/lib/domain';
import { Icon } from '../shared/ui';

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
      </section>
    </>
  );
}
