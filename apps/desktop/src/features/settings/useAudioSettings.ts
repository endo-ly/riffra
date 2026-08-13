import { useCallback, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioDriverConfig, AudioStatus } from '@/model/domain';
import { reconcileAudioSettings } from '@/features/settings/audio-settings';
import { audioCommandSucceeded, isEmergencyMuteActive } from '@/shared/audio/audio-safety';
import type { AudioApi } from '@/native/native-api';

interface UseAudioOptions {
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
}

export function useAudioSettings(api: AudioApi, options: UseAudioOptions) {
  const {
    recoverAudioDevice,
    setAudioDriver,
    enableMidiListening,
    disableMidiListening,
    setEmergencyMute,
  } = api;
  const { audio, setAudio } = options;
  const [audioPreferenceMessage, setAudioPreferenceMessage] = useState<string | null>(null);

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
          sampleRate: config.sampleRate ?? nextAudio.sampleRate ?? 48_000,
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
    setAudio(await enableMidiListening());
  }, [enableMidiListening, setAudio]);

  const disableMidi = useCallback(async () => {
    setAudio(await disableMidiListening());
  }, [disableMidiListening, setAudio]);

  const toggleMute = useCallback(async () => {
    const muted = !isEmergencyMuteActive(audio);
    setAudio(await setEmergencyMute(muted));
  }, [audio, setAudio, setEmergencyMute]);

  return {
    audioPreferenceMessage,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    disableMidi,
    toggleMute,
  };
}
