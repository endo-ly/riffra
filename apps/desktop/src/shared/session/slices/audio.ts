import type {
  AudioDeviceProbe,
  AudioDriverConfig,
  AudioStatus,
  DeviceChannels,
} from '@/model/generated';
import type { NativeApi } from '@/native/native-api';
import { startingAudioStatus } from '@/shared/audio/audio-defaults';
import { isAudioEngineUsable, isEmergencyMuteActive } from '@/shared/audio/audio-safety';
import { reconcileAudioSettings } from '@/shared/audio/reconcile-settings';
import { notify } from '@/shared/notify';

export interface AudioSlice {
  status: AudioStatus;
  probe: AudioDeviceProbe;
  setStatus(status: AudioStatus): void;
  refreshDevices(): Promise<AudioDeviceProbe>;
  probeChannels(driver: string, inputDevice: string, outputDevice: string): Promise<DeviceChannels>;
  recover(): Promise<AudioStatus>;
  selectDriver(config: AudioDriverConfig): Promise<AudioStatus>;
  enableMidi(): Promise<void>;
  toggleMute(): Promise<void>;
  resetFeedback(): Promise<void>;
  reset(): void;
}

export function createAudioSlice(
  api: NativeApi,
  get: () => AudioSlice,
  update: (patch: Partial<AudioSlice>) => void,
): AudioSlice {
  let generation = 0;
  const initial = () => ({
    status: startingAudioStatus(),
    probe: { drivers: [], refreshedAtMs: 0, message: 'Audio device list has not been refreshed.' },
  });
  return {
    ...initial(),
    setStatus(status) {
      if (status.feedbackSuspected && !get().status.feedbackSuspected)
        notify.error('Feedback detected', {
          message: 'Output was muted to protect your hearing and speakers.',
        });
      update({ status });
    },
    reset: () => {
      generation += 1;
      update(initial());
    },
    async refreshDevices() {
      const current = generation;
      const probe = await api.probeAudioDevices();
      if (generation === current) update({ probe });
      return probe;
    },
    probeChannels: (driver, inputDevice, outputDevice) =>
      api.probeDeviceChannels(driver, inputDevice, outputDevice),
    async recover() {
      const current = generation;
      const status = await api.recoverAudioDevice();
      if (generation === current) get().setStatus(status);
      return status;
    },
    async selectDriver(config) {
      const current = generation;
      const status = await api.setAudioDriver(config);
      if (generation === current) {
        get().setStatus(status);
        if (isAudioEngineUsable(status)) {
          const effective = reconcileAudioSettings(
            {
              driver: config.driver,
              sampleRate: config.sampleRate ?? status.sampleRate ?? 48000,
              bufferSize: config.bufferSize ?? status.bufferSize ?? 256,
            },
            status,
          );
          if (effective.message)
            notify.info('Audio settings adjusted', { message: effective.message });
        }
      }
      return status;
    },
    async enableMidi() {
      const current = generation;
      const status = await api.enableMidiListening();
      if (generation === current) get().setStatus(status);
    },
    async toggleMute() {
      const current = generation;
      try {
        const status = await api.setEmergencyMute(!isEmergencyMuteActive(get().status));
        if (generation === current) get().setStatus(status);
      } catch (error) {
        if (generation === current) notify.error('Mute failed', { cause: error });
      }
    },
    async resetFeedback() {
      const current = generation;
      try {
        const status = await api.resetFeedbackProtection();
        if (generation === current) get().setStatus(status);
      } catch (error) {
        if (generation === current) notify.error('Feedback reset failed', { cause: error });
      }
    },
  };
}
