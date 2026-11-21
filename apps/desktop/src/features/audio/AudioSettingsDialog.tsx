import { errorText } from '@/shared/errors';
import { useSession } from '@/session/session-context';
import { useEffect, useRef, useState } from 'react';
import type { AudioStatus } from '@/model/generated';
import {
  audioBufferSizeOptions,
  audioSampleRateOptions,
  createAudioSettingsDraft,
  includeEffectiveOption,
  isAudioSettingsDraftValid,
  mergeAudioStatusChannels,
  mergeDeviceChannels,
  normalizeAudioSettingsDraft,
  sameAudioSettingsDraft,
  selectDriverForDraft,
} from '@/features/audio/model/settings-draft';
import { isAudioEngineUsable } from '@/model/audio-status';
import { Dialog } from '@/shared/ui/Dialog';
import surface from '@/shared/ui/Surface.module.css';
import styles from './AudioSettingsDialog.module.css';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';

export function AudioSettingsDialog({ onClose }: { onClose: () => void }) {
  const audioSlice = useSession((state) => state.audio);
  const recording = useSession((state) => state.recording);
  const audio = audioSlice.status;
  const probe = audioSlice.probe;
  const safeMode = useSession((state) => state.project.boot?.safeMode ?? false);
  const recordingActive =
    audio.recording.active ||
    audio.recording.processing ||
    recording.commandPending ||
    recording.stopPending;
  const onRefresh = audioSlice.refreshDevices;
  const onProbeChannels = audioSlice.probeChannels;
  const onApply = audioSlice.selectDriver;
  const onRecover = audioSlice.recover;

  const [initialProbe] = useState(() => mergeAudioStatusChannels(probe, audio));
  const [availableProbe, setAvailableProbe] = useState(initialProbe);
  const [initialDraft] = useState(() => createAudioSettingsDraft(audio, initialProbe));
  const [draft, setDraft] = useState(initialDraft);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [applying, setApplying] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const inFlightDeviceKeys = useRef(new Set<string>());
  const previousProbe = useRef(probe);
  const audioRef = useRef(audio);
  audioRef.current = audio;
  const firstField = useRef<HTMLSelectElement | null>(null);

  useEffect(() => {
    if (previousProbe.current === probe) return;
    previousProbe.current = probe;
    const nextProbe = mergeAudioStatusChannels(probe, audioRef.current);
    setAvailableProbe(nextProbe);
    setDraft((current) => normalizeAudioSettingsDraft(current, nextProbe));
  }, [probe]);

  useEffect(() => {
    setAvailableProbe((current) => mergeAudioStatusChannels(current, audioRef.current));
  }, [
    audio.driver,
    audio.inputChannels,
    audio.inputDevice,
    audio.outputChannels,
    audio.outputDevice,
  ]);

  useEffect(() => {
    const focusTimer = window.setTimeout(() => firstField.current?.focus(), 0);
    return () => window.clearTimeout(focusTimer);
  }, []);

  // Startup discovery is passive: it never opens a device, so the selected
  // interface's channel names are only known after a per-device detail probe.
  // Resolve them lazily for the currently selected interface so the channel
  // selector and draft validation have real names without probing every
  // device (especially every ASIO driver) during startup.
  useEffect(() => {
    if (safeMode) return;
    const driver = draft.driver;
    const inputDevice = draft.inputDevice;
    if (driver === '' || inputDevice == null) return;
    const active = availableProbe.drivers.find((candidate) => candidate.name === driver);
    const selected = active?.inputs.find((device) => device.name === inputDevice);
    const activeInputChannels =
      audio.driver === driver && audio.inputDevice === inputDevice ? audio.inputChannels : [];
    if (selected == null || selected.channels.length > 0 || activeInputChannels.length > 0) return;
    if (refreshing || applying || recovering) return;
    const outputDevice = draft.outputDevice ?? inputDevice;
    const deviceKey = `${driver}\u0000${inputDevice}\u0000${outputDevice}`;
    if (inFlightDeviceKeys.current.has(deviceKey)) return;
    inFlightDeviceKeys.current.add(deviceKey);
    let cancelled = false;
    setError(null);
    void onProbeChannels(driver, inputDevice, outputDevice)
      .then((detail) => {
        if (cancelled) return;
        if (detail.inputChannels.length === 0) {
          setError('Device channel details returned no input channels.');
          return;
        }
        setAvailableProbe((current) => mergeDeviceChannels(current, detail));
      })
      .catch((reason) => {
        if (!cancelled)
          setError(errorText(reason) || 'Device channel details could not be loaded.');
      })
      .finally(() => {
        inFlightDeviceKeys.current.delete(deviceKey);
      });
    return () => {
      cancelled = true;
    };
  }, [
    safeMode,
    draft.driver,
    draft.inputDevice,
    draft.outputDevice,
    availableProbe,
    refreshing,
    applying,
    recovering,
    audio.driver,
    audio.inputChannels,
    audio.inputDevice,
    onProbeChannels,
  ]);

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
  const changed = !sameAudioSettingsDraft(draft, initialDraft);
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
      const effectiveProbe = mergeAudioStatusChannels(nextProbe, audio);
      setAvailableProbe(effectiveProbe);
      setDraft((current) => normalizeAudioSettingsDraft(current, effectiveProbe));
    } catch (reason) {
      setError(errorText(reason) || 'Audio device refresh failed.');
    } finally {
      setRefreshing(false);
    }
  };

  const recover = async () => {
    setRecovering(true);
    setError(null);
    try {
      const nextAudio = await onRecover();
      if (!isAudioEngineUsable(nextAudio)) setError(nextAudio.message);
    } catch (reason) {
      setError(errorText(reason) || 'Audio recovery failed.');
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
      if (isAudioEngineUsable(nextAudio)) onClose();
      else setError(nextAudio.message);
    } catch (reason) {
      setError(errorText(reason) || 'Audio settings could not be applied.');
    } finally {
      setApplying(false);
    }
  };

  const setDriver = (name: string) => {
    const nextDriver = availableProbe.drivers.find((driver) => driver.name === name);
    if (nextDriver) setDraft(selectDriverForDraft(draft, nextDriver));
  };

  return (
    <Dialog
      label="Audio Settings"
      title="Audio Settings"
      subtitle={audio.driver ?? 'No driver'}
      icon="speaker"
      closeDisabled={applying}
      onClose={close}
      footer={
        <>
          <div className={styles.footerLeft}>
            <button className={surface.textButton} onClick={() => void refresh()} disabled={busy}>
              {refreshing ? 'Refreshing…' : 'Refresh devices'}
            </button>
            {(audio.state === 'faulted' || audio.state === 'offline') && (
              <button
                className={surface.textButton}
                onClick={() => void recover()}
                disabled={busy || safeMode}
              >
                {recovering ? 'Recovering…' : 'Recover Audio'}
              </button>
            )}
          </div>
          <div className={styles.footerRight}>
            <button className={surface.quiet} onClick={close} disabled={applying}>
              Cancel
            </button>
            <button
              className={surface.primary}
              onClick={() => void apply()}
              disabled={applyDisabled}
            >
              {applying ? 'Applying…' : 'Apply'}
            </button>
          </div>
        </>
      }
    >
      <div className={styles.body}>
        <div className={`${styles.status} ${styles[audio.state]}`}>
          <span />
          <strong>{statusLabel(audio.state)}</strong>
          <span className={styles.statusCopy}>
            {audio.driver ?? 'No driver'} · {audio.sampleRate?.toLocaleString() ?? '—'} Hz ·{' '}
            {audio.bufferSize ?? '—'} samples
          </span>
        </div>

        {safeMode && (
          <InlineMessage tone="warning">Safe mode blocks audio driver changes.</InlineMessage>
        )}

        {activeDriver?.accessMode === 'driverManaged' && (
          <InlineMessage tone="info">
            {activeDriver.name} manages the audio device itself. Depending on the driver and
            sample-rate settings, other applications using the same device may be paused while the
            audio engine is running.
          </InlineMessage>
        )}

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
        {error && <InlineMessage tone="danger">{error}</InlineMessage>}
      </div>
    </Dialog>
  );
}

function statusLabel(state: AudioStatus['state']): string {
  return state === 'ready' ? 'Ready' : state[0].toUpperCase() + state.slice(1);
}
