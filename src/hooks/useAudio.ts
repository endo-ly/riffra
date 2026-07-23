import { useCallback, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioDriverConfig, AudioStatus, CreativeSession, RecordingAsset } from '@/lib/domain';
import { reconcileAudioSettings } from '@/lib/audio-settings';
import { audioCommandSucceeded, isOutputMuted } from '@/lib/audio-safety';
import type { NativeApi } from '@/native/native-api';

interface UseAudioOptions {
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  session: CreativeSession | null;
  setSession: (session: CreativeSession) => void;
  setRecordings: (recordings: RecordingAsset[]) => void;
}

export function useAudio(api: NativeApi, options: UseAudioOptions) {
  const {
    recoverAudioDevice,
    bootstrap,
    setAudioDriver,
    enableMidiListening,
    disableMidiListening,
    setEmergencyMute,
    startArrangeRecording,
    stopArrangeRecording,
    listRecordings,
  } = api;
  const { audio, setAudio, setSession, setRecordings } = options;
  const [audioPreferenceMessage, setAudioPreferenceMessage] = useState<string | null>(null);
  const [recordCountdown, setRecordCountdown] = useState<number | null>(null);
  const [recordingCommandPending, setRecordingCommandPending] = useState(false);
  const recordingCommandLock = useRef(false);

  const recoverAudio = useCallback(async () => {
    setAudioPreferenceMessage(null);
    setAudio(await recoverAudioDevice());
  }, [recoverAudioDevice, setAudio]);

  const selectAudioDriver = useCallback(
    async (config: AudioDriverConfig) => {
      const nextAudio = await setAudioDriver(config);
      setAudio(nextAudio);
      if (!audioCommandSucceeded(nextAudio)) return;
      const effective = reconcileAudioSettings(
        {
          driver: config.driver,
          sampleRate: config.sampleRate ?? nextAudio.sampleRate ?? 48_000,
          bufferSize: config.bufferSize ?? nextAudio.bufferSize ?? 256,
        },
        nextAudio,
      );
      setAudioPreferenceMessage(effective.message);
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
    const muted = !isOutputMuted(audio);
    setAudio(await setEmergencyMute(muted));
  }, [audio, setAudio, setEmergencyMute]);

  const startRecordingNow = useCallback(
    async (recordingSessionId?: string) => {
      if (recordingCommandLock.current) return;
      recordingCommandLock.current = true;
      setRecordingCommandPending(true);
      try {
        const nextAudio = await startArrangeRecording(recordingSessionId);
        setAudio(nextAudio);
        setRecordings(await listRecordings());
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
    },
    [listRecordings, setAudio, setRecordings, startArrangeRecording],
  );

  const toggleRecording = useCallback(async () => {
    if (recordingCommandLock.current) return;
    if (!audio.recording.active) {
      await startRecordingNow();
      return;
    }
    recordingCommandLock.current = true;
    setRecordingCommandPending(true);
    try {
      setAudio(await stopArrangeRecording());
      setSession((await bootstrap()).session);
      setRecordings(await listRecordings());
    } finally {
      recordingCommandLock.current = false;
      setRecordingCommandPending(false);
    }
  }, [
    audio.recording.active,
    bootstrap,
    listRecordings,
    setAudio,
    setRecordings,
    setSession,
    startRecordingNow,
    stopArrangeRecording,
  ]);

  return {
    audioPreferenceMessage,
    setAudioPreferenceMessage,
    recordCountdown,
    setRecordCountdown,
    recordingCommandPending,
    setRecordingCommandPending,
    recordingCommandLock,
    recoverAudio,
    selectAudioDriver,
    enableMidi,
    disableMidi,
    toggleMute,
    startRecordingNow,
    toggleRecording,
  };
}
