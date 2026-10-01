import { useCallback, useEffect, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioDeviceProbe, AudioDriverConfig, AudioStatus } from '@/model/domain';
import { reconcileAudioSettings } from '@/features/audio/audio-settings';
import { audioCommandSucceeded, isEmergencyMuteActive } from '@/shared/audio/audio-safety';
import type { AudioApi } from '@/native/native-api';
interface UseAudioOptions {
  hostGeneration?: number;
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
}

export function useAudioSettings(api: AudioApi, options: UseAudioOptions) {
  const {
    recoverAudioDevice,
    setAudioDriver,
    enableMidiListening,
    setEmergencyMute,
    resetFeedbackProtection,
  } = api;
  const { audio, hostGeneration = 0, setAudio } = options;
  const [audioPreferenceMessage, setAudioPreferenceMessage] = useState<string | null>(null);
  const [deviceProbe, setDeviceProbe] = useState<AudioDeviceProbe>({
    drivers: [],
    refreshedAtMs: 0,
    message: 'Audio device list has not been refreshed.',
  });

  useEffect(() => {
    setAudioPreferenceMessage(null);
    setDeviceProbe({
      drivers: [],
      refreshedAtMs: 0,
      message: 'Audio device list has not been refreshed.',
    });
  }, [hostGeneration]);
  const refreshAudioDevices = useCallback(async () => {
    const nextProbe = await api.probeAudioDevices();
    setDeviceProbe(nextProbe);
    return nextProbe;
  }, [api]);
  const probeAudioChannels = useCallback(
    async (driver: string, inputDevice: string, outputDevice: string) => {
      const channels = await api.probeDeviceChannels(driver, inputDevice, outputDevice);
      return channels;
    },
    [api],
  );

  const recoverAudio = useCallback(async () => {
    setAudioPreferenceMessage(null);
    const nextAudio = await recoverAudioDevice();
    setAudio(nextAudio);
    return nextAudio;
  }, [recoverAudioDevice, setAudio]);
  const selectAudioDriver = useCallback(
    async (config: AudioDriverConfig) => {
      const nextAudio = await setAudioDriver(config);
      setAudio(nextAudio);
      if (!audioCommandSucceeded(nextAudio)) return nextAudio;
      const effective = reconcileAudioSettings(
        {
          driver: config.driver,
          sampleRate: config.sampleRate ?? nextAudio.sampleRate ?? 48000,
          bufferSize: config.bufferSize ?? nextAudio.bufferSize ?? 256,
        },
        nextAudio,
      );
      setAudioPreferenceMessage(effective.message);
      return nextAudio;
    },
    [setAudio, setAudioDriver],
  );

  const enableMidi = useCallback(async () => {
    const nextAudio = await enableMidiListening();
    setAudio(nextAudio);
  }, [enableMidiListening, setAudio]);
  const toggleMute = useCallback(async () => {
    const muted = !isEmergencyMuteActive(audio);
    const nextAudio = await setEmergencyMute(muted);
    setAudio(nextAudio);
  }, [audio, setAudio, setEmergencyMute]);
  const resetFeedback = useCallback(async () => {
    const nextAudio = await resetFeedbackProtection();
    setAudio(nextAudio);
  }, [resetFeedbackProtection, setAudio]);
  return {
    audioPreferenceMessage,
    deviceProbe,
    refreshAudioDevices,
    probeAudioChannels,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    toggleMute,
    resetFeedback,
  };
}
