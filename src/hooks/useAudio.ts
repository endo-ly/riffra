import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioDriverConfig, AudioStatus, CreativeSession, RecordingAsset } from '@/lib/domain';
import { reconcileAudioSettings } from '@/lib/audio-settings';
import { audioCommandSucceeded, isOutputMuted } from '@/lib/audio-safety';
import { decideRecordingToggle } from '@/lib/recording';
import { COUNT_IN_BEAT_MS } from '@/constants';
import type { NativeApi } from '@/native/native-api';

interface UseAudioOptions {
  audio: AudioStatus;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  session: CreativeSession | null;
  setRecordings: (recordings: RecordingAsset[]) => void;
}

export function useAudio(api: NativeApi, options: UseAudioOptions) {
  const {
    recoverAudioDevice,
    setAudioDriver,
    enableMidiListening,
    disableMidiListening,
    setEmergencyMute,
    startRecording,
    stopRecording,
    listRecordings,
  } = api;
  const { audio, setAudio, session, setRecordings } = options;
  const [audioPreferenceMessage, setAudioPreferenceMessage] = useState<string | null>(null);
  const [recordCountdown, setRecordCountdown] = useState<number | null>(null);
  const [recordingCommandPending, setRecordingCommandPending] = useState(false);
  const recordingCommandLock = useRef(false);

  const recoverAudio = useCallback(async () => {
    setAudioPreferenceMessage(null);
    setAudio(await recoverAudioDevice());
  }, []);

  const selectAudioDriver = useCallback(async (config: AudioDriverConfig) => {
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
  }, []);

  const enableMidi = useCallback(async () => {
    setAudio(await enableMidiListening());
  }, []);

  const disableMidi = useCallback(async () => {
    setAudio(await disableMidiListening());
  }, []);

  const toggleMute = useCallback(async () => {
    const muted = !isOutputMuted(audio);
    setAudio(await setEmergencyMute(muted));
  }, [audio]);

  const startRecordingNow = useCallback(async () => {
    if (recordingCommandLock.current) return;
    recordingCommandLock.current = true;
    setRecordingCommandPending(true);
    try {
      const nextAudio = await startRecording();
      setAudio(nextAudio);
      setRecordings(await listRecordings());
    } finally {
      recordingCommandLock.current = false;
      setRecordingCommandPending(false);
    }
  }, []);

  const toggleRecording = useCallback(async () => {
    const decision = decideRecordingToggle({
      commandPending: recordingCommandLock.current,
      countdown: recordCountdown,
      recordingActive: audio.recording.active,
      countInBeats: session?.settings.countInBeats ?? 0,
    });
    switch (decision.kind) {
      case 'ignore':
        return;
      case 'cancelCountdown':
        setRecordCountdown(null);
        return;
      case 'stop':
        recordingCommandLock.current = true;
        setRecordingCommandPending(true);
        try {
          setAudio(await stopRecording());
          setRecordings(await listRecordings());
        } finally {
          recordingCommandLock.current = false;
          setRecordingCommandPending(false);
        }
        return;
      case 'startCountdown':
        setRecordCountdown(decision.beats);
        return;
      case 'startNow':
        await startRecordingNow();
        return;
    }
  }, [audio.recording.active, recordCountdown, session?.settings.countInBeats, startRecordingNow]);

  useEffect(() => {
    if (recordCountdown === null) return;
    if (recordCountdown === 0) {
      setRecordCountdown(null);
      void startRecordingNow();
      return;
    }
    const timer = window.setTimeout(
      () => setRecordCountdown((current) => (current === null ? null : current - 1)),
      COUNT_IN_BEAT_MS,
    );
    return () => window.clearTimeout(timer);
  }, [recordCountdown, startRecordingNow]);

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
