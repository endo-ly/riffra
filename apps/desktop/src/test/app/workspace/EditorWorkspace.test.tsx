// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import type { CreativeSession } from '@/model/generated';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/model/asset-id';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { renderWorkspace } from '@/test/support/workspace';

afterEach(cleanup);

describe('EditorWorkspace', () => {
  it('creates an empty MIDI clip from an instrument lane and opens its editor', async () => {
    const session = defaultSession();
    const track = {
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-empty',
      name: 'Instrument Empty',
      kind: 'instrument' as const,
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off' as const,
      midiInput: {},
      effects: [],
    };
    session.arrangement.tracks.push(track);
    const createdSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: [
          {
            instrumentControlEvents: [],
            id: 'clip:created-empty',
            name: 'MIDI Clip',
            trackId: track.id,
            startTick: 480,
            durationTicks: 1_920,
            notes: [],
            events: [],
            muted: false,
            loopEnabled: false,
          },
        ],
      },
    };
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let createArgs: Parameters<FakeNativeApi['createMidiClip']> | undefined;
    api.createMidiClip = async (...args) => {
      createArgs = args;
      api.calls.push('createMidiClip');
      return {
        ...api.mutationResult(createdSession),
        createdEntityIds: { midiClips: [createdSession.arrangement.midiClips[0]!.id] },
      };
    };
    const { container } = renderWorkspace(api, { session });
    const lane = container.querySelector(`[data-track-id="${track.id}"] > div[class*="lane_"]`)!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 100, top: 0, right: 800, bottom: 80, width: 700, height: 80 }),
    });

    fireEvent.doubleClick(lane, { clientX: 196, clientY: 40 });

    await waitFor(() => expect(createArgs).toBeDefined());
    expect(createArgs?.[0]).toBe(track.id);
    expect(createArgs?.[2]).toBe(session.arrangement.timebase.ppq * 4);
    expect(await screen.findByLabelText('MIDI Editor')).toBeInTheDocument();
    expect(container.querySelector('[data-clip-id="clip:created-empty"]')).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('uses the active Time Selection for an empty MIDI clip', async () => {
    const session = defaultSession();
    const track = {
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-selection',
      name: 'Instrument Selection',
      kind: 'instrument' as const,
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off' as const,
      midiInput: {},
      effects: [],
    };
    session.arrangement.tracks.push(track);
    const createdSession: CreativeSession = {
      ...session,
      arrangement: {
        ...session.arrangement,
        midiClips: [
          {
            instrumentControlEvents: [],
            id: 'clip:selection-created',
            name: 'Selection Clip',
            trackId: track.id,
            startTick: 960,
            durationTicks: 960,
            notes: [],
            events: [],
            muted: false,
            loopEnabled: false,
          },
        ],
      },
    };
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    let createArgs: Parameters<FakeNativeApi['createMidiClip']> | undefined;
    api.createMidiClip = async (...args) => {
      createArgs = args;
      api.calls.push('createMidiClip');
      return {
        ...api.mutationResult(createdSession),
        createdEntityIds: { midiClips: [createdSession.arrangement.midiClips[0]!.id] },
      };
    };
    const { container } = renderWorkspace(api, { session });
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 2_400, top: 0, bottom: 30, right: 2_400, height: 30 }),
    });

    fireEvent.pointerDown(ruler, { clientX: 96 });
    fireEvent.pointerMove(window, { clientX: 192 });
    fireEvent.pointerUp(window, { clientX: 192 });

    const lane = container.querySelector(`[data-track-id="${track.id}"] > div[class*="lane_"]`)!;
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 100, top: 0, right: 800, bottom: 80, width: 700, height: 80 }),
    });
    fireEvent.doubleClick(lane, { clientX: 200, clientY: 40 });

    await waitFor(() => expect(createArgs).toBeDefined());
    expect(createArgs?.[1]).toBe(960);
    expect(createArgs?.[2]).toBe(960);
    expect(await screen.findByLabelText('MIDI Editor')).toBeInTheDocument();
  });

  it('keeps the active MIDI Clip while selecting Audio or appending another MIDI Clip', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push(
      {
        externalAudioSourceTrackIds: [],
        panLaw: 'equalPower' as const,
        id: 'track:audio-selection',
        name: 'Audio Selection',
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
        id: 'track:midi-selection',
        name: 'MIDI Selection',
        kind: 'instrument',
        gainDb: 0,
        pan: 0,
        muted: false,
        solo: false,
        armed: false,
        monitoring: 'off',
        midiInput: {},
        effects: [],
      },
    );
    session.arrangement.audioClips.push({
      id: 'clip:audio-selection',
      name: 'Audio Selection Clip',
      trackId: 'track:audio-selection',
      assetId: toAssetId('asset:audio-selection'),
      startTick: 0,
      sourceRange: { start: 0, end: 48_000 },
      sourceSampleRate: 48_000,
      timelineDuration: { frames: 48_000, sampleRate: 48_000 },
      gainDb: 0,
      pan: 0,
      fadeIn: { frames: 0, sampleRate: 48_000 },
      fadeOut: { frames: 0, sampleRate: 48_000 },
      loopEnabled: false,
      muted: false,
      fadeShape: 'equalPower',
      takeVariant: 'raw',
    });
    session.arrangement.midiClips.push(
      {
        instrumentControlEvents: [],
        id: 'clip:midi-selection-a',
        name: 'MIDI Selection A',
        trackId: 'track:midi-selection',
        startTick: 0,
        durationTicks: 1_920,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
      {
        instrumentControlEvents: [],
        id: 'clip:midi-selection-b',
        name: 'MIDI Selection B',
        trackId: 'track:midi-selection',
        startTick: 2_400,
        durationTicks: 1_920,
        notes: [],
        events: [],
        muted: false,
        loopEnabled: false,
      },
    );
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:midi-selection-a"]')!);
    await screen.findByLabelText('MIDI Editor');
    fireEvent.click(container.querySelector('[data-clip-id="clip:midi-selection-b"]')!, {
      ctrlKey: true,
    });
    expect(screen.getByLabelText('MIDI Editor')).toHaveAttribute(
      'data-midi-editor-clip-id',
      'clip:midi-selection-a',
    );

    fireEvent.click(container.querySelector('[data-clip-id="clip:audio-selection"]')!);
    expect(screen.getByLabelText('MIDI Editor')).toBeInTheDocument();
    expect(screen.getByLabelText('MIDI Editor')).toHaveAttribute(
      'data-midi-editor-clip-id',
      'clip:midi-selection-a',
    );

    fireEvent.click(container.querySelector('[data-clip-id="clip:midi-selection-b"]')!);
    expect(screen.getByLabelText('MIDI Editor')).toHaveAttribute(
      'data-midi-editor-clip-id',
      'clip:midi-selection-b',
    );
  });

  it('reports clips when confirming Track deletion and closes the selected Track Devices', async () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:instrument-delete',
      name: 'Instrument Delete',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:midi-delete',
      name: 'MIDI Delete',
      trackId: 'track:instrument-delete',
      startTick: 0,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const afterDelete = structuredClone(session);
    afterDelete.arrangement.tracks = afterDelete.arrangement.tracks.filter(
      (track) => track.id !== 'track:instrument-delete',
    );
    afterDelete.arrangement.midiClips = [];
    api.removeTrack = vi.fn().mockResolvedValue(api.mutationResult(afterDelete));
    renderWorkspace(api, { session });

    // Act
    fireEvent.click(screen.getByLabelText('Instrument Delete track menu'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open Devices' }));
    expect(screen.getByRole('region', { name: 'Arrange lower area' })).toHaveAttribute(
      'data-view',
      'devices',
    );
    fireEvent.click(screen.getByLabelText('Instrument Delete track menu'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));

    // Assert
    expect(screen.getByText(/This also removes 1 Clip from the Timeline/)).toBeInTheDocument();
    expect(screen.getByText(/Source assets will be kept\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Delete Track' }));
    await waitFor(() =>
      expect(screen.queryByRole('region', { name: 'Arrange lower area' })).not.toBeInTheDocument(),
    );
  });

  it('opens Devices from the track menu with available and disabled plugins', () => {
    // Arrange
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:guitar',
      name: 'Guitar',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [
        {
          id: 'device:amp',
          name: 'Amplitube',
          bypassed: false,
          plugin: {
            path: 'C:/Plugins/Amplitube.vst3',
            parameterValues: [],
            disabledPlaceholder: false,
          },
        },
        {
          id: 'device:disabled',
          name: 'Disabled Effect',
          bypassed: false,
          plugin: {
            path: 'C:/Plugins/Disabled.vst3',
            parameterValues: [],
            disabledPlaceholder: true,
          },
        },
      ],
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    renderWorkspace(api, { session });

    // Act
    fireEvent.click(screen.getByLabelText('Guitar track menu'));
    expect(screen.getByRole('menu')).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Guitar track menu'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open Devices' }));

    // Assert
    expect(screen.getByRole('region', { name: 'Arrange lower area' })).toHaveAttribute(
      'data-view',
      'devices',
    );
    expect(screen.getByRole('button', { name: 'Amplitube' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Disabled Effect' })).toBeInTheDocument();
  });

  it('resizes, collapses, maximizes, and closes the Arrange lower area', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:navigation',
      name: 'Navigation Track',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
    session.arrangement.midiClips.push({
      instrumentControlEvents: [],
      id: 'clip:navigation',
      name: 'Navigation Clip',
      trackId: 'track:navigation',
      startTick: 1_920,
      durationTicks: 1_920,
      notes: [],
      events: [],
      muted: false,
      loopEnabled: false,
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    const { container } = renderWorkspace(api, { session });

    fireEvent.doubleClick(container.querySelector('[data-clip-id="clip:navigation"]')!);
    await screen.findByLabelText('MIDI Editor');

    const workspace = screen.getByLabelText('Arrange timeline');
    Object.defineProperty(workspace, 'clientHeight', {
      configurable: true,
      value: 600,
    });
    const resizeHandle = screen.getByRole('separator', { name: 'Resize Arrange lower area' });
    // The default lower area height is 380: dragging 210px further down lands below the
    // 180px minimum and collapses instead of shrinking.
    fireEvent.pointerDown(resizeHandle, { clientY: 500 });
    fireEvent.pointerMove(window, { clientY: 710 });
    expect(screen.getByRole('button', { name: 'Restore lower area' })).toBeInTheDocument();
    fireEvent.pointerUp(window, { clientY: 710 });

    const lowerArea = screen.getByRole('region', { name: 'Arrange lower area' });
    fireEvent.click(screen.getByRole('button', { name: 'Restore lower area' }));
    expect(lowerArea.querySelector('[hidden]')).not.toBeInTheDocument();

    fireEvent.pointerDown(resizeHandle, { clientY: 500 });
    fireEvent.pointerMove(window, { clientY: -200 });
    fireEvent.pointerUp(window, { clientY: -200 });
    expect(lowerArea).toHaveStyle('--lower-height: 558px');

    fireEvent.click(screen.getByRole('button', { name: 'Collapse lower area' }));
    expect(screen.getByRole('button', { name: 'Restore lower area' })).toBeInTheDocument();
    expect(lowerArea.querySelector('[hidden]')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Maximize lower area' }));
    expect(screen.getByRole('button', { name: 'Restore lower area size' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close lower area' }));
    expect(screen.queryByRole('region', { name: 'Arrange lower area' })).not.toBeInTheDocument();
  });

  it('shows the Mixer and the MIDI editor in the shared lower area', async () => {
    const session = defaultSession();
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:focused',
      name: 'Focused Instrument',
      kind: 'instrument',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
    session.arrangement.tracks.push({
      externalAudioSourceTrackIds: [],
      panLaw: 'equalPower' as const,
      id: 'track:mixer',
      name: 'Mix Track',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      effects: [],
    });
    const api = createFakeNativeApi({ bootstrapState: { canonical: canonicalState(session) } });
    renderWorkspace(api, { session, focusedTrackId: 'track:focused' });

    const mixerToggle = screen.getByRole('button', { name: 'Mixer' });
    fireEvent.click(mixerToggle);

    const lowerArea = screen.getByRole('region', { name: 'Arrange lower area' });
    expect(lowerArea.querySelector('[data-mixer-panel]')).toBeInTheDocument();
    const trackChannel = screen.getByRole('article', { name: 'Mix Track mixer channel' });
    expect(screen.getByRole('complementary', { name: 'Master mixer channel' })).toBeInTheDocument();

    fireEvent.click(within(trackChannel).getByTitle('Open Track Devices'));
    await waitFor(() => expect(lowerArea).toHaveAttribute('data-view', 'devices'));
    fireEvent.click(mixerToggle);
    expect(lowerArea).toHaveAttribute('data-view', 'mixer');
    fireEvent.click(mixerToggle);
    expect(lowerArea).toHaveAttribute('data-view', 'devices');
  });

  it('routes Space to the shared transport controller from the Arrange editor', async () => {
    const api = createFakeNativeApi();
    renderWorkspace(api);
    const workspace = screen.getByLabelText('Arrange timeline');

    fireEvent.keyDown(workspace, { key: ' ' });

    await waitFor(() => expect(api.calls).toContain('playTimeline'));
  });
});
