// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { renderWorkspace } from '@/test/support/workspace';

afterEach(cleanup);

describe('LiveInput', () => {
  it('plays the focused instrument from the keyboard and drum pads on their MIDI channels', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:live',
      name: 'Live Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:live',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:live',
      name: 'Live Clip',
      trackId: 'track:live',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    const setLiveTarget = vi.spyOn(api, 'setLiveMidiTarget');
    const { container } = renderWorkspace(api, { session });
    fireEvent.click(screen.getByText('Live Keys'));
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });

    // Act: play without opening any panel.
    fireEvent.click(computerKeyboard);
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });

    // Assert
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
    expect(computerKeyboard).toHaveTextContent('C4');
    expect(screen.queryByRole('region', { name: 'Play Surface' })).not.toBeInTheDocument();
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:live', [0x90, 60, 100]));
    expect(setLiveTarget).toHaveBeenLastCalledWith('track:live');

    // Act: the Play Surface opens over the MIDI Editor and returns to it.
    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:live"]')!);
    const lowerArea = screen.getByRole('region', { name: 'Arrange lower area' });
    fireEvent.click(screen.getByRole('button', { name: 'Play Surface' }));

    // Assert
    expect(lowerArea).toHaveAttribute('data-view', 'playSurface');
    expect(screen.getByRole('region', { name: 'Play Surface' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Drum Pads' }));
    const pad = screen.getByRole('gridcell', {
      name: 'Crash Cymbal 1 (MIDI 49, key Q)',
    });
    Object.defineProperty(pad, 'setPointerCapture', {
      configurable: true,
      value: vi.fn(),
    });
    fireEvent.pointerDown(pad, { pointerId: 1 });
    fireEvent.keyDown(window, { key: 'q' });
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:live', [0x99, 49, 100]));
    fireEvent.keyUp(window, { key: 'q' });
    expect(
      sendMidi.mock.calls.filter(([, bytes]) => bytes[0] === 0x89 && bytes[1] === 49),
    ).toHaveLength(0);
    fireEvent.pointerUp(pad, { pointerId: 1 });
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:live', [0x89, 49, 0]));

    fireEvent.click(screen.getByRole('button', { name: 'Play Surface' }));
    expect(lowerArea).toHaveAttribute('data-view', 'midiEditor');
  });

  it('keeps a held performance key away from the Arrange zoom shortcuts', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:live',
      name: 'Live Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:live',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:live',
      name: 'Live Clip',
      trackId: 'track:live',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    const { container } = renderWorkspace(api, { session });
    const scroller = container.querySelector('[class*="scroller"]') as HTMLElement;
    Object.defineProperty(scroller, 'clientWidth', { value: 1408, configurable: true });
    fireEvent.click(screen.getByText('Live Keys'));
    fireEvent.click(screen.getByRole('button', { name: 'Computer keyboard input' }));
    const zoom = screen.getByRole('group', { name: 'Timeline zoom' });
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });
    const before = zoom.textContent;

    // Act: hold the typing keys, which are also Arrange zoom shortcuts.
    fireEvent.keyDown(window, { key: 'f' });
    fireEvent.keyDown(window, { key: 'f', repeat: true });
    fireEvent.keyUp(window, { key: 'f' });
    fireEvent.keyDown(window, { key: 'z', repeat: true });

    // Assert: the repeat never zooms, sounds one note, or shifts the octave.
    expect(zoom.textContent).toBe(before);
    expect(computerKeyboard).toHaveTextContent('C4');
    expect(sendMidi.mock.calls.filter(([, bytes]) => bytes[0] === 0x90)).toHaveLength(1);
  });

  it('never arms live input on a track that cannot play', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:audio',
        name: 'Audio Track',
        kind: 'audio',
        gainDb: 0,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        effects: [],
      },
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:live',
        name: 'Live Keys',
        kind: 'instrument',
        gainDb: 0,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        instrument: {
          id: 'device:live',
          name: 'Fake Synth',
          bypassed: false,
          midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
          source: {
            type: 'vst3',
            path: 'C:\\Plugins\\FakeSynth.vst3',
            parameterValues: [],
            disabledPlaceholder: false,
          },
        },
        effects: [],
      },
    );
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    renderWorkspace(api, { session });
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });

    // Act: select an Audio Track, which cannot receive performance MIDI.
    fireEvent.click(screen.getByText('Audio Track'));

    // Assert: the toggle reports the reason instead of showing itself engaged.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    expect(computerKeyboard).toHaveAttribute(
      'title',
      expect.stringContaining('No Track is focused.'),
    );

    // Act: ask for live input there, then focus an Instrument Track.
    fireEvent.click(computerKeyboard);
    fireEvent.click(screen.getByText('Live Keys'));

    // Assert: the Instrument Track does not inherit an input the user never armed.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });
    expect(sendMidi).not.toHaveBeenCalled();

    // Act: arm it on the Instrument Track, then move through a Track that cannot play.
    fireEvent.click(computerKeyboard);
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByText('Audio Track'));

    // Assert: leaving a playable Track releases live input, label included.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    expect(computerKeyboard).not.toHaveTextContent('C4');

    // Act: return to the Instrument Track.
    fireEvent.click(screen.getByText('Live Keys'));

    // Assert: live input stayed off instead of coming back on its own.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'false');
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });
    expect(sendMidi).not.toHaveBeenCalled();
  });

  it('keeps live input armed while moving between playable tracks', async () => {
    // Arrange
    const session = defaultSession();
    const playable = (id: string, name: string) => ({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id,
      name,
      kind: 'instrument' as const,
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off' as const,
      midiInput: {},
      instrument: {
        id: `device:${id}`,
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3' as const,
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    session.arrangement.tracks.push(playable('track:a', 'Keys A'), playable('track:b', 'Keys B'));
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const sendMidi = vi.spyOn(api, 'sendMidiToTrack');
    renderWorkspace(api, { session });
    fireEvent.click(screen.getByText('Keys A'));
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });
    fireEvent.click(computerKeyboard);

    // Act: move to the other Instrument Track.
    fireEvent.click(screen.getByText('Keys B'));
    fireEvent.keyDown(window, { key: 'a' });
    fireEvent.keyUp(window, { key: 'a' });

    // Assert: the input the user turned on followed the focus.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
    await waitFor(() => expect(sendMidi).toHaveBeenCalledWith('track:b', [0x90, 60, 100]));
  });

  it('toggles live input once for a held computer keyboard shortcut', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:live',
      name: 'Live Keys',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      instrument: {
        id: 'device:live',
        name: 'Fake Synth',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'vst3',
          path: 'C:\\Plugins\\FakeSynth.vst3',
          parameterValues: [],
          disabledPlaceholder: false,
        },
      },
      effects: [],
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    renderWorkspace(api, { session });
    fireEvent.click(screen.getByText('Live Keys'));
    const computerKeyboard = screen.getByRole('button', { name: 'Computer keyboard input' });

    // Act: hold Ctrl+Shift+K.
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true, shiftKey: true, repeat: true });
    fireEvent.keyUp(window, { key: 'k', ctrlKey: true, shiftKey: true });

    // Assert: the press turned it on and the repeats did not turn it back off.
    expect(computerKeyboard).toHaveAttribute('aria-pressed', 'true');
  });
});
