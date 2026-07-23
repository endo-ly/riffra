import { useCallback, useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { AudioDriverConfig, AudioStatus, CreativeSession, RecordingAsset } from '@/lib/domain';
import { reconcileAudioSettings } from '@/lib/audio-settings';
import { audioCommandSucceeded, isOutputMuted } from '@/lib/audio-safety';
import { decideRecordingToggle } from '@/lib/recording';
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
    startRecording,
    recordAnotherTake,
    stopRecording,
    listRecordings,
    playTimeline,
    stopTimeline,
  } = api;
  const { audio, setAudio, session, setSession, setRecordings } = options;
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
        const nextAudio = recordingSessionId
          ? await recordAnotherTake(recordingSessionId)
          : await startRecording();
        setAudio(nextAudio);
        await playTimeline();
        setRecordings(await listRecordings());
      } finally {
        recordingCommandLock.current = false;
        setRecordingCommandPending(false);
      }
    },
    [listRecordings, playTimeline, recordAnotherTake, setAudio, setRecordings, startRecording],
  );

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
          await stopTimeline();
          setSession((await bootstrap()).session);
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
  }, [
    audio.recording.active,
    bootstrap,
    listRecordings,
    recordCountdown,
    session?.settings.countInBeats,
    setAudio,
    setRecordings,
    setSession,
    startRecordingNow,
    stopRecording,
    stopTimeline,
  ]);

  useEffect(() => {
    if (recordCountdown === null) return;
    if (recordCountdown === 0) {
      setRecordCountdown(null);
      void startRecordingNow();
      return;
    }
    const timer = window.setTimeout(
      () => setRecordCountdown((current) => (current === null ? null : current - 1)),
      (60_000 * 4) /
        ((session?.arrangement.timebase.bpm ?? 120) *
          (session?.arrangement.timebase.timeSignatureDenominator ?? 4)),
    );
    return () => window.clearTimeout(timer);
  }, [
    recordCountdown,
    session?.arrangement.timebase.bpm,
    session?.arrangement.timebase.timeSignatureDenominator,
    startRecordingNow,
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
