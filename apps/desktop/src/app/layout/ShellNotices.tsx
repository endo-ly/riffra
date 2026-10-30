import { useState } from 'react';
import { useSession } from '@/shared/session/session-context';
import { selectHostReady } from '@/shared/session/selectors';
import { isEmergencyMuteActive } from '@/shared/audio/audio-safety';
import { useAudioFeedbackSuspected } from '@/shared/audio/audio-meters';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import surface from '@/shared/ui/Surface.module.css';
import type { useAppUpdater } from '@/app/runtime/useAppUpdater';
import styles from './ShellNotices.module.css';

export function ShellNotices({ appUpdater }: { appUpdater: ReturnType<typeof useAppUpdater> }) {
  const [restoreCandidate, setRestoreCandidate] = useState<string | null>(null);
  const project = useSession((state) => state.project);
  const audio = useSession((state) => state.audio.status);
  const jobs = useSession((state) => state.jobs);
  const boot = project.boot!;
  const hostConnected = useSession(selectHostReady);
  const projectSwitching = project.switching;
  const { restoreRecovery, keepRecovered: dismissRecovery } = project;
  const { active: backgroundJob, cancel: cancelActiveJob } = jobs;
  const liveFeedbackSuspected = useAudioFeedbackSuspected();
  const feedbackSuspected = liveFeedbackSuspected || audio.feedbackSuspected;
  const isMuted = isEmergencyMuteActive(audio);
  return (
    <>
      <div className={styles.bannerStack}>
        {isMuted && (
          <div className={`${styles.runtimeBanner} ${styles.dangerBanner}`} role="status">
            <strong>OUTPUT MUTED</strong>
            <span>
              {feedbackSuspected
                ? 'Feedback was detected, so all output was silenced automatically.'
                : 'All audio output is silenced.'}
            </span>
          </div>
        )}
        {boot.safeMode && (
          <div className={`${styles.shellNotice} ${styles.safeModeNotice}`} role="status">
            <strong>SAFE MODE</strong>
            <span>
              External VST3, MIDI input, driver changes, live preview and new recordings are
              isolated. Project open, library access, offline analysis, render and export remain
              available. Restart without <code>--safe-mode</code> to reconnect devices.
            </span>
          </div>
        )}
        {boot.recovery.recoveredFromGeneration && boot.recovery.recoveryCandidates.length > 0 && (
          <div className={`${styles.shellNotice} ${styles.recoveryNotice}`} role="status">
            <strong>RECOVERY CHOICE</strong>
            <span>
              The current session was recovered from an autosave generation. Choose a previous
              stable generation if needed.
            </span>
            <div className={styles.recoveryActions}>
              {boot.recovery.recoveryCandidates.slice(0, 5).map((candidate) => (
                <button
                  className={surface.textButton}
                  key={candidate.fileName}
                  disabled={!hostConnected || projectSwitching}
                  onClick={() => setRestoreCandidate(candidate.fileName)}
                >
                  {candidate.projectName ?? 'Untitled'} ·{' '}
                  {new Date(candidate.updatedAtMs).toLocaleString('ja-JP')}
                </button>
              ))}
              <button
                className={surface.textButton}
                disabled={projectSwitching}
                onClick={dismissRecovery}
              >
                Keep recovered session
              </button>
            </div>
          </div>
        )}
        {appUpdater.availableVersion && (
          <div className={`${styles.shellNotice} ${styles.updateNotice}`} role="status">
            <strong>UPDATE AVAILABLE</strong>
            <span>
              Riffra {appUpdater.availableVersion} can be installed. The session stays untouched
              until you restart.
            </span>
            <div className={styles.recoveryActions}>
              <button
                className={surface.textButton}
                disabled={appUpdater.installing}
                onClick={() => void appUpdater.install()}
              >
                {appUpdater.installing ? 'Installing…' : 'Update & Restart'}
              </button>
            </div>
          </div>
        )}
        {backgroundJob && (
          <div className={styles.runtimeBanner}>
            <strong>
              {backgroundJob.kind.toUpperCase()} JOB · {backgroundJob.state.toUpperCase()}
            </strong>
            <span>{backgroundJob.message}</span>
            {['queued', 'running', 'cancelling'].includes(backgroundJob.state) && (
              <button
                className={surface.textButton}
                disabled={!hostConnected}
                onClick={() => void cancelActiveJob()}
              >
                Cancel
              </button>
            )}
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
      </div>

      {restoreCandidate && (
        <ConfirmDialog
          title="Restore stable generation"
          message={`Restore autosave generation ${restoreCandidate}? The current session will become the selected stable copy.`}
          confirmLabel="Restore"
          onConfirm={() => {
            void restoreRecovery(restoreCandidate);
            setRestoreCandidate(null);
          }}
          onCancel={() => setRestoreCandidate(null)}
        />
      )}
    </>
  );
}
