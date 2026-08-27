import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioDeviceProbe, AudioDriverConfig, AudioStatus } from '@/model/domain';
import { reconcileAudioSettings } from '@/features/audio/audio-settings';
import { audioCommandSucceeded, isEmergencyMuteActive } from '@/shared/audio/audio-safety';
import type { AudioApi } from '@/native/native-api';
import { HostConnectionChangedError } from '@/native/invoke';

interface UseAudioOptions {
  hostGeneration?: number;
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
}

export function useAudioSettings(api: AudioApi, options: UseAudioOptions) {
  const { recoverAudioDevice, setAudioDriver, enableMidiListening, setEmergencyMute } = api;
  const { audio, hostGeneration = 0, setAudio } = options;
  const currentHostGeneration = useRef(hostGeneration);
  currentHostGeneration.current = hostGeneration;
  const [audioPreferenceMessage, setAudioPreferenceMessage] = useState<string | null>(null);
  const [deviceProbe, setDeviceProbe] = useState<AudioDeviceProbe>({
    drivers: [],
    refreshedAtMs: 0,
    message: 'Audio device list has not been refreshed.',
  });

  useEffect(() => {
    currentHostGeneration.current = hostGeneration;
    setAudioPreferenceMessage(null);
    setDeviceProbe({
      drivers: [],
      refreshedAtMs: 0,
      message: 'Audio device list has not been refreshed.',
    });
  }, [hostGeneration]);

  const assertCurrent = useCallback((generation: number) => {
    if (currentHostGeneration.current !== generation) throw new HostConnectionChangedError();
  }, []);

  const refreshAudioDevices = useCallback(async () => {
    const requestGeneration = hostGeneration;
    const nextProbe = await api.probeAudioDevices();
    assertCurrent(requestGeneration);
    setDeviceProbe(nextProbe);
    return nextProbe;
  }, [api, assertCurrent, hostGeneration]);

  const probeAudioChannels = useCallback(
    async (driver: string, inputDevice: string, outputDevice: string) => {
      const requestGeneration = hostGeneration;
      const channels = await api.probeDeviceChannels(driver, inputDevice, outputDevice);
      assertCurrent(requestGeneration);
      return channels;
    },
    [api, assertCurrent, hostGeneration],
  );

  const recoverAudio = useCallback(async () => {
    const requestGeneration = hostGeneration;
    setAudioPreferenceMessage(null);
    const nextAudio = await recoverAudioDevice();
    assertCurrent(requestGeneration);
    setAudio(nextAudio);
    return nextAudio;
  }, [assertCurrent, hostGeneration, recoverAudioDevice, setAudio]);

  const selectAudioDriver = useCallback(
    async (config: AudioDriverConfig) => {
      const requestGeneration = hostGeneration;
      const nextAudio = await setAudioDriver(config);
      assertCurrent(requestGeneration);
      setAudio(nextAudio);
      if (!audioCommandSucceeded(nextAudio)) return nextAudio;
      const effective = reconcileAudioSettings(
        {
          driver: config.driver,
          sampleRate: config.sampleRate ?? nextAudio.sampleRate ?? 48_000,
          bufferSize: config.bufferSize ?? nextAudio.bufferSize ?? 256,
        },
        nextAudio,
      );
      setAudioPreferenceMessage(effective.message);
      return nextAudio;
    },
    [assertCurrent, hostGeneration, setAudio, setAudioDriver],
  );

  const enableMidi = useCallback(async () => {
    const requestGeneration = hostGeneration;
    const nextAudio = await enableMidiListening();
    if (currentHostGeneration.current === requestGeneration) setAudio(nextAudio);
  }, [enableMidiListening, hostGeneration, setAudio]);

  const toggleMute = useCallback(async () => {
    const requestGeneration = hostGeneration;
    const muted = !isEmergencyMuteActive(audio);
    const nextAudio = await setEmergencyMute(muted);
    if (currentHostGeneration.current === requestGeneration) setAudio(nextAudio);
  }, [audio, hostGeneration, setAudio, setEmergencyMute]);

  return {
    audioPreferenceMessage,
    deviceProbe,
    refreshAudioDevices,
    probeAudioChannels,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    toggleMute,
  };
}
