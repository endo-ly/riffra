import { useCallback, useEffect, useState } from 'react';
import type { Track } from '@/model/domain';
import type { AudioApi } from '@/native/native-api';
import { HostConnectionChangedError, logNativeError } from '@/native/invoke';
import {
  MUSICAL_TYPING_DEFAULT_OCTAVE,
  MUSICAL_TYPING_DEFAULT_VELOCITY,
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
} from './musical-typing';
import { useDrumPads } from './useDrumPads';
import { useMusicalTyping } from './useMusicalTyping';

type LivePlayMode = 'keys' | 'pads';

interface LivePlaySettings {
  mode: LivePlayMode;
  octave: number;
  velocity: number;
}

/** Why the focused track cannot be played live, or `ready` when it can. */
export type LivePlayAvailability =
  | 'ready'
  | 'noTrack'
  | 'notInstrument'
  | 'noInstrument'
  | 'instrumentMissing'
  | 'runtimeUnavailable';

/** The single reason live input cannot reach the focused track. */
export const LIVE_PLAY_BLOCKED_REASON: Record<LivePlayAvailability, string> = {
  ready: '',
  noTrack: 'No Track is focused.',
  notInstrument: 'Only Instrument Tracks can be played live.',
  noInstrument: 'No instrument assigned.',
  instrumentMissing: 'Instrument unavailable.',
  runtimeUnavailable: 'Audio runtime unavailable.',
};

const DEFAULT_SETTINGS: LivePlaySettings = {
  mode: 'keys',
  octave: MUSICAL_TYPING_DEFAULT_OCTAVE,
  velocity: MUSICAL_TYPING_DEFAULT_VELOCITY,
};

interface UseLivePlayOptions {
  /** The Focused Instrument Track that live input plays. */
  track: Track | null;
  runtimeReady: boolean;
  hostConnected: boolean;
  missingDeviceIds: string[];
  /** Whether the on-screen Play Surface is shown. */
  surfaceOpen: boolean;
  api: Pick<AudioApi, 'sendMidiToTrack' | 'setLiveMidiTarget'>;
}

/**
 * Live playing of the Focused Instrument Track from the computer keyboard and
 * the on-screen Play Surface. Computer keyboard input works whether or not the
 * surface is shown; mode, octave and velocity are kept per track.
 */
export function useLivePlay({
  track,
  runtimeReady,
  hostConnected,
  missingDeviceIds,
  surfaceOpen,
  api,
}: UseLivePlayOptions) {
  const [computerKeyboard, setComputerKeyboard] = useState(false);
  const [settingsByTrack, setSettingsByTrack] = useState<Record<string, LivePlaySettings>>({});
  const [message, setMessage] = useState<string | null>(null);
  const settings = (track && settingsByTrack[track.id]) || DEFAULT_SETTINGS;
  const targetTrackId = track?.kind === 'instrument' ? track.id : null;

  const availability: LivePlayAvailability = !track
    ? 'noTrack'
    : track.kind !== 'instrument'
      ? 'notInstrument'
      : !track.instrument
        ? 'noInstrument'
        : track.instrument.source.type === 'vst3' &&
            (track.instrument.source.disabledPlaceholder ||
              missingDeviceIds.includes(track.instrument.id))
          ? 'instrumentMissing'
          : !runtimeReady
            ? 'runtimeUnavailable'
            : 'ready';
  const canPlay = availability === 'ready';

  // The runtime monitors the played track with low latency while anything can play it.
  const liveTarget = canPlay && (computerKeyboard || surfaceOpen) ? targetTrackId : null;
  useEffect(() => {
    if (hostConnected) void api.setLiveMidiTarget(liveTarget);
  }, [api, hostConnected, liveTarget]);
  useEffect(
    () => () => {
      if (hostConnected)
        void api.setLiveMidiTarget(null).catch((error: unknown) => {
          if (!(error instanceof HostConnectionChangedError))
            logNativeError('setLiveMidiTarget')(error);
        });
    },
    [api, hostConnected],
  );

  const updateSettings = useCallback(
    (patch: Partial<LivePlaySettings>) => {
      if (!track) return;
      setSettingsByTrack((current) => ({
        ...current,
        [track.id]: { ...(current[track.id] ?? DEFAULT_SETTINGS), ...patch },
      }));
    },
    [track],
  );
  const shiftOctave = (delta: number) =>
    updateSettings({
      octave: Math.max(
        MUSICAL_TYPING_MIN_OCTAVE,
        Math.min(MUSICAL_TYPING_MAX_OCTAVE, settings.octave + delta),
      ),
    });

  const sendMidi = useCallback(
    async (trackId: string, bytes: number[]) => {
      const status = await api.sendMidiToTrack(trackId, bytes);
      if (status) setMessage(status.message);
    },
    [api],
  );

  const melodic = useMusicalTyping({
    enabled: canPlay && computerKeyboard && settings.mode === 'keys',
    targetTrackId,
    octave: settings.octave,
    velocity: settings.velocity,
    sendMidi,
    onOctaveChange: shiftOctave,
  });
  const drums = useDrumPads({
    enabled: canPlay && computerKeyboard && settings.mode === 'pads',
    targetTrackId,
    velocity: settings.velocity,
    sendMidi,
  });

  // A track that cannot play refuses to turn live input on, so focusing another
  // Track never leaves input armed behind a toolbar that shows it off. Turning
  // it off always works.
  const toggleComputerKeyboard = useCallback(
    () => setComputerKeyboard((enabled) => (enabled ? false : canPlay)),
    [canPlay],
  );

  // Losing the ability to play releases live input, so it cannot come back armed
  // on its own once another Track can play. Moving between playable Tracks keeps
  // the input the user turned on.
  useEffect(() => {
    if (!canPlay) setComputerKeyboard(false);
  }, [canPlay]);

  const setMode = (mode: LivePlayMode) => {
    melodic.releaseHeldNotes();
    drums.releaseAll();
    updateSettings({ mode });
  };

  return {
    track,
    availability,
    canPlay,
    /** Why live input cannot reach the focused track, or `null` when it can. */
    blockedReason: canPlay ? null : LIVE_PLAY_BLOCKED_REASON[availability],
    computerKeyboard,
    toggleComputerKeyboard,
    settings,
    setMode,
    shiftOctave,
    setVelocity: (velocity: number) => updateSettings({ velocity }),
    melodic,
    drums,
    activeNotes: settings.mode === 'pads' ? drums.activeNotes : melodic.activeNotes,
    message,
  };
}

export type LivePlay = ReturnType<typeof useLivePlay>;
