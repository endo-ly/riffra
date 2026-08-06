import { useEffect, useRef, useState } from 'react';
import type { AudioDeviceProbe, AudioDriverConfig, AudioStatus } from '@/lib/domain';
import {
  audioBufferSizeOptions,
  audioSampleRateOptions,
  createAudioSettingsDraft,
  includeEffectiveOption,
  isAudioSettingsDraftValid,
  normalizeAudioSettingsDraft,
  selectDriverForDraft,
} from '@/lib/audio-settings';
import { audioCommandSucceeded } from '@/lib/audio-safety';
import styles from './AudioSettingsDialog.module.css';

interface AudioSettingsDialogProps {
  open: boolean;
  audio: AudioStatus;
  probe: AudioDeviceProbe;
  safeMode: boolean;
  recordingActive: boolean;

  onClose: () => void;
  onRefresh: () => Promise<AudioDeviceProbe>;
  onApply: (config: AudioDriverConfig) => Promise<AudioStatus>;
  onRecover: () => Promise<AudioStatus>;
}

export function AudioSettingsDialog({
  open,
  audio,
  probe,
  safeMode,
  recordingActive,
  onClose,
  onRefresh,
  onApply,
  onRecover,
}: AudioSettingsDialogProps) {
  const [availableProbe, setAvailableProbe] = useState(probe);
  const [draft, setDraft] = useState(() => createAudioSettingsDraft(audio, probe));
  const [initialDraft, setInitialDraft] = useState(() => createAudioSettingsDraft(audio, probe));
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [applying, setApplying] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const wasOpen = useRef(false);
  const returnFocus = useRef<HTMLElement | null>(null);
  const firstField = useRef<HTMLSelectElement | null>(null);

  useEffect(() => {
    setAvailableProbe(probe);
    if (open) setDraft((current) => normalizeAudioSettingsDraft(current, probe));
  }, [open, probe]);

  useEffect(() => {
    if (open && !wasOpen.current) {
      returnFocus.current =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      const nextDraft = createAudioSettingsDraft(audio, probe);
      setAvailableProbe(probe);
      setDraft(nextDraft);
      setInitialDraft(nextDraft);
      setError(null);
      const focusTimer = window.setTimeout(() => firstField.current?.focus(), 0);
      wasOpen.current = true;
      return () => window.clearTimeout(focusTimer);
    }
    if (!open && wasOpen.current) {
      wasOpen.current = false;
      returnFocus.current?.focus();
      returnFocus.current = null;
    }
  }, [audio, open, probe]);

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !applying) {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [applying, onClose, open]);

  if (!open) return null;

  const activeDriver =
    availableProbe.drivers.find((driver) => driver.name === draft.driver) ?? null;
  const inputInfo =
    activeDriver?.inputs.find((device) => device.name === draft.inputDevice) ?? null;
  const inputChannels = inputInfo?.channels ?? [];
  const pairedDevices =
    activeDriver?.inputs.filter((device) =>
      activeDriver.outputs.some((output) => output.name === device.name),
    ) ?? [];
  const rateOptions = includeEffectiveOption(
    draft.sampleRate ?? audio.sampleRate ?? 48_000,
    audioSampleRateOptions,
  );
  const bufferOptions = includeEffectiveOption(
    draft.bufferSize ?? audio.bufferSize ?? 256,
    audioBufferSizeOptions,
  );
  const hasDevices = availableProbe.drivers.some((driver) =>
    driver.devicePairing === 'sameDevice'
      ? driver.inputs.some((input) => driver.outputs.some((output) => output.name === input.name))
      : driver.inputs.length > 0 && driver.outputs.length > 0,
  );
  const activeDriverHasDevices =
    activeDriver != null &&
    (activeDriver.devicePairing === 'sameDevice'
      ? pairedDevices.length > 0
      : activeDriver.inputs.length > 0 && activeDriver.outputs.length > 0);
  const changed = !sameDraft(draft, initialDraft);
  const valid = isAudioSettingsDraftValid(draft, availableProbe);
  const busy = refreshing || applying || recovering;
  const fieldsDisabled = busy || safeMode;
  const applyDisabled =
    !changed || !valid || busy || recordingActive || safeMode || audio.state === 'starting';

  const close = () => {
    if (applying) return;
    onClose();
  };

  const refresh = async () => {
    setRefreshing(true);
    setError(null);
    try {
      const nextProbe = await onRefresh();
      setAvailableProbe(nextProbe);
      setDraft((current) => normalizeAudioSettingsDraft(current, nextProbe));
    } catch (reason) {
      setError(errorMessage(reason, 'Audio device refresh failed.'));
    } finally {
      setRefreshing(false);
    }
  };

  const recover = async () => {
    setRecovering(true);
    setError(null);
    try {
      const nextAudio = await onRecover();
      if (!audioCommandSucceeded(nextAudio)) setError(nextAudio.message);
    } catch (reason) {
      setError(errorMessage(reason, 'Audio recovery failed.'));
    } finally {
      setRecovering(false);
    }
  };

  const apply = async () => {
    const nextDraft = normalizeAudioSettingsDraft(draft, availableProbe);
    if (!isAudioSettingsDraftValid(nextDraft, availableProbe)) return;
    setDraft(nextDraft);
    setApplying(true);
    setError(null);
    try {
      const nextAudio = await onApply(nextDraft);
      if (audioCommandSucceeded(nextAudio)) onClose();
      else setError(nextAudio.message);
    } catch (reason) {
      setError(errorMessage(reason, 'Audio settings could not be applied.'));
    } finally {
      setApplying(false);
    }
  };

  const setDriver = (name: string) => {
    const nextDriver = availableProbe.drivers.find((driver) => driver.name === name);
    if (nextDriver) setDraft(selectDriverForDraft(draft, nextDriver));
  };

  return (
    <div className={styles.backdrop} onMouseDown={close}>
      <section
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="audio-settings-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className={styles.header}>
          <div>
            <span className="eyebrow">SETTINGS</span>
            <h2 id="audio-settings-title">Audio Settings</h2>
          </div>
          <button
            className={styles.closeButton}
            onClick={close}
            disabled={applying}
            aria-label="Close"
          >
            ×
          </button>
        </header>

        <div className={styles.body}>
          <div className={`${styles.status} ${styles[audio.state]}`}>
            <span />
            <strong>{statusLabel(audio.state)}</strong>
            <span className={styles.statusCopy}>
              {audio.driver ?? 'No driver'} · {audio.sampleRate?.toLocaleString() ?? '—'} Hz ·{' '}
              {audio.bufferSize ?? '—'} samples
            </span>
          </div>

          {safeMode && <p className={styles.notice}>Safe Mode blocks audio-driver changes.</p>}

          {!hasDevices ? (
            <p className={styles.empty}>No audio devices are currently available.</p>
          ) : (
            <>
              <section className={styles.section}>
                <h3>DRIVER</h3>
                <label className={styles.field}>
                  <span>Audio driver</span>
                  <select
                    ref={firstField}
                    aria-label="Audio driver"
                    value={draft.driver}
                    disabled={fieldsDisabled}
                    onChange={(event) => setDriver(event.target.value)}
                  >
                    {availableProbe.drivers.map((driver) => (
                      <option key={driver.name} value={driver.name}>
                        {driver.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label className={styles.field}>
                  <span>Sample rate</span>
                  <select
                    aria-label="Sample rate"
                    value={draft.sampleRate ?? ''}
                    disabled={fieldsDisabled}
                    onChange={(event) =>
                      setDraft((current) => ({
                        ...current,
                        sampleRate: Number(event.target.value),
                      }))
                    }
                  >
                    {rateOptions.map((rate) => (
                      <option key={rate} value={rate}>
                        {rate.toLocaleString()} Hz
                      </option>
                    ))}
                  </select>
                </label>
                <label className={styles.field}>
                  <span>Buffer size</span>
                  <select
                    aria-label="Buffer size"
                    value={draft.bufferSize ?? ''}
                    disabled={fieldsDisabled}
                    onChange={(event) =>
                      setDraft((current) => ({
                        ...current,
                        bufferSize: Number(event.target.value),
                      }))
                    }
                  >
                    {bufferOptions.map((buffer) => (
                      <option key={buffer} value={buffer}>
                        {buffer} samples
                      </option>
                    ))}
                  </select>
                </label>
              </section>

              {activeDriverHasDevices ? (
                <section className={styles.section}>
                  <h3>WINDOWS DEVICES</h3>
                  {activeDriver?.devicePairing === 'sameDevice' ? (
                    <label className={styles.field}>
                      <span>Audio device</span>
                      <select
                        aria-label="Audio device"
                        value={draft.inputDevice ?? ''}
                        disabled={fieldsDisabled}
                        onChange={(event) =>
                          setDraft((current) =>
                            normalizeAudioSettingsDraft(
                              {
                                ...current,
                                inputDevice: event.target.value || null,
                                outputDevice: event.target.value || null,
                              },
                              availableProbe,
                            ),
                          )
                        }
                      >
                        {pairedDevices.map((device) => (
                          <option key={device.name} value={device.name}>
                            {device.name}
                          </option>
                        ))}
                      </select>
                    </label>
                  ) : (
                    <label className={styles.field}>
                      <span>Input device</span>
                      <select
                        aria-label="Input device"
                        value={draft.inputDevice ?? ''}
                        disabled={fieldsDisabled}
                        onChange={(event) =>
                          setDraft((current) =>
                            normalizeAudioSettingsDraft(
                              { ...current, inputDevice: event.target.value || null },
                              availableProbe,
                            ),
                          )
                        }
                      >
                        {activeDriver?.inputs.map((device) => (
                          <option key={device.name} value={device.name}>
                            {device.name}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                  <label className={styles.field}>
                    <span>Input channel</span>
                    <select
                      aria-label="Input channel"
                      value={draft.inputChannel}
                      disabled={fieldsDisabled || inputChannels.length === 0}
                      onChange={(event) =>
                        setDraft((current) => ({
                          ...current,
                          inputChannel: Number(event.target.value),
                        }))
                      }
                    >
                      {inputChannels.length === 0 && (
                        <option value="">No input channels available</option>
                      )}
                      {inputChannels.map((channel) => (
                        <option key={channel.index} value={channel.index}>
                          {channel.name}
                        </option>
                      ))}
                    </select>
                  </label>
                  {activeDriver?.devicePairing === 'independent' && (
                    <label className={styles.field}>
                      <span>Output device</span>
                      <select
                        aria-label="Output device"
                        value={draft.outputDevice ?? ''}
                        disabled={fieldsDisabled}
                        onChange={(event) =>
                          setDraft((current) => ({
                            ...current,
                            outputDevice: event.target.value || null,
                          }))
                        }
                      >
                        {activeDriver?.outputs.map((device) => (
                          <option key={device.name} value={device.name}>
                            {device.name}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </section>
              ) : (
                <p className={styles.empty}>No audio devices are available for this driver.</p>
              )}
            </>
          )}
        </div>

        {error && (
          <p className={styles.error} role="alert">
            {error}
          </p>
        )}

        <footer className={styles.footer}>
          <div className={styles.footerLeft}>
            <button className="text-button" onClick={() => void refresh()} disabled={busy}>
              {refreshing ? 'Refreshing…' : 'Refresh devices'}
            </button>
            {(audio.state === 'faulted' || audio.state === 'offline') && (
              <button
                className="text-button"
                onClick={() => void recover()}
                disabled={busy || safeMode}
              >
                {recovering ? 'Recovering…' : 'Recover Audio'}
              </button>
            )}
          </div>
          <div className={styles.footerRight}>
            <button className="quiet" onClick={close} disabled={applying}>
              Cancel
            </button>
            <button className="primary" onClick={() => void apply()} disabled={applyDisabled}>
              {applying ? 'Applying…' : 'Apply'}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}

function sameDraft(left: AudioDriverConfig, right: AudioDriverConfig): boolean {
  return (
    left.driver === right.driver &&
    left.inputDevice === right.inputDevice &&
    left.inputChannel === right.inputChannel &&
    left.outputDevice === right.outputDevice &&
    left.sampleRate === right.sampleRate &&
    left.bufferSize === right.bufferSize
  );
}

function errorMessage(reason: unknown, fallback: string): string {
  return reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : fallback;
}

function statusLabel(state: AudioStatus['state']): string {
  return state === 'ready' ? 'Ready' : state[0].toUpperCase() + state.slice(1);
}
