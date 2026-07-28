// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { WorkspaceArrange } from '@/components';
import {
  defaultSession,
  toAssetId,
  type CreativeSession,
  type TransportStatus,
} from '@/lib/domain';
import { FakeNativeApi } from '@/native/native-api-fake';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';

afterEach(cleanup);

function Harness({
  api,
  initialSession,
}: {
  api: FakeNativeApi;
  initialSession?: CreativeSession;
}) {
  const initial = initialSession ?? defaultSession();
  initial.workspace = 'arrange';
  const [session, setSession] = useState<CreativeSession>(initial);
  const [selection, setSelection] = useState<ArrangeSelection>({ kind: 'none' });
  return (
    <WorkspaceArrange
      session={session}
      setSession={setSession}
      selection={selection}
      setSelection={setSelection}
      api={api}
    />
  );
}

describe('WorkspaceArrange', () => {
  it('creates the first audio track when an audio Asset is dropped on an empty timeline', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    const { container } = render(<Harness api={api} />);
    const empty = screen.getByText('Start arranging').parentElement!;
    const timeline = empty.closest('[class*="timeline"]')!;
    Object.defineProperty(timeline, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 800, top: 0, bottom: 180, right: 800, height: 180 }),
    });

    fireEvent.drop(empty, {
      clientX: 172,
      dataTransfer: {
        getData: () =>
          JSON.stringify({
            id: toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a'),
            name: 'Take',
            kind: 'audio',
          }),
      },
    });

    await waitFor(() => expect(api.calls).toContain('addAudioClipToArrangement'));
    expect(await screen.findByText('Audio 1')).toBeInTheDocument();
    const clipName = await screen.findByText('Take');
    await waitFor(() => expect(container.querySelector('svg')).toBeInTheDocument());
    const clip = clipName.closest('button')!;
    fireEvent.click(clip);
    expect(clip).toHaveAttribute('aria-pressed', 'true');
    fireEvent.keyDown(window, { key: 'd', ctrlKey: true });
    await waitFor(() => expect(api.calls).toContain('pasteTimelineClips'));
    expect(await screen.findByText('Take copy')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'c', ctrlKey: true });
    fireEvent.keyDown(window, { key: 'v', ctrlKey: true });
    await waitFor(() => expect(api.calls).toContain('pasteTimelineClips'));
  });

  it('seeks the native timeline from the musical ruler', () => {
    const api = new FakeNativeApi();
    render(<Harness api={api} />);
    const ruler = screen.getByLabelText('Timeline ruler');
    Object.defineProperty(ruler, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 1472, top: 0, bottom: 30, right: 1472, height: 30 }),
    });

    fireEvent.pointerDown(ruler, { clientX: 92 });

    expect(api.calls).toContain('seekTimeline');
  });

  it('deletes an empty Audio Track from its Track Header', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    const userConfirmed = vi.spyOn(window, 'confirm').mockReturnValue(true);
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Audio Track' }));
    fireEvent.click(await screen.findByLabelText('Audio 1 track menu'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    await waitFor(() => expect(api.calls).toContain('removeTrack'));
    expect(userConfirmed).toHaveBeenCalledWith(expect.stringContaining('Source Audio Assets'));
    expect(screen.queryByText('Audio 1')).not.toBeInTheDocument();
  });

  it('edits Track Automation with one Session commit per gesture', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);

    fireEvent.click(screen.getByRole('button', { name: 'Audio Track' }));
    fireEvent.click(await screen.findByText('Audio 1'));
    fireEvent.click(screen.getByRole('button', { name: 'Automation' }));
    const lane = screen.getByLabelText('Audio 1 volume automation');
    Object.defineProperty(lane, 'getBoundingClientRect', {
      value: () => ({ left: 0, width: 1200, top: 0, bottom: 84, right: 1200, height: 84 }),
    });

    fireEvent.pointerDown(lane, { button: 0, clientX: 120, clientY: 42 });
    await waitFor(() => expect(api.calls).toContain('setTrackAutomation'));
    const point = screen.getByRole('button', { name: /volume .* at tick/ });
    const commitsBeforeDrag = api.calls.filter((call) => call === 'setTrackAutomation').length;
    fireEvent.pointerDown(point, { button: 0, clientX: 120, clientY: 42 });
    fireEvent.pointerMove(window, { clientX: 180, clientY: 24 });
    expect(api.calls.filter((call) => call === 'setTrackAutomation')).toHaveLength(
      commitsBeforeDrag,
    );
    fireEvent.pointerUp(window, { clientX: 180, clientY: 24 });
    await waitFor(() =>
      expect(api.calls.filter((call) => call === 'setTrackAutomation')).toHaveLength(
        commitsBeforeDrag + 1,
      ),
    );
  });

  it('keeps an unavailable clip on the timeline and labels its missing source', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    const assetId = toAssetId('asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a');
    session.arrangement.tracks.push({
      id: 'track:audio',
      name: 'Audio',
      kind: 'audio',
      gainDb: 0,
      pan: 0,
      muted: false,
      solo: false,
      armed: false,
      monitoring: 'off',
      midiInput: {},
      rack: { devices: [], macros: [] },
    });
    session.arrangement.audioClips.push({
      id: 'clip:missing',
      name: 'Lost Take',
      trackId: 'track:audio',
      assetId,
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
      takeVariant: 'raw',
    });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const status: TransportStatus = {
      type: 'transportStatus',
      state: 'stopped',
      revision: session.arrangement.revision,
      timelineTick: 0,
      timelineSample: 0,
      audioClockSample: 0,
      sampleRate: 48_000,
      sequence: 1,
      recordingPhase: 'idle',
      recordingStartTick: 0,
      recordingCurrentTick: 0,
      recordingPassOrdinal: 0,
      armedTrackIds: [],
      clockGeneration: 1,
      discontinuity: 1,
      unavailableClipIds: ['clip:missing'],
      missingDeviceIds: [],
    };
    api.onTransportStatus = (callback) => {
      queueMicrotask(() => callback(status));
      return () => undefined;
    };

    render(<Harness api={api} initialSession={session} />);

    expect(await screen.findByText('Lost Take')).toBeInTheDocument();
    expect((await screen.findAllByText('MISSING SOURCE')).length).toBeGreaterThan(0);
    expect(document.querySelector('[data-clip-id="clip:missing"]')).toBeInTheDocument();
  });

  it('disables the timeline loop from the ruler context menu', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const ruler = screen.getByLabelText('Timeline ruler');
    fireEvent.contextMenu(ruler, { clientX: 200, clientY: 8, button: 2 });
    const clearLoop = await screen.findByRole('menuitem', { name: 'Clear Loop' });
    fireEvent.click(clearLoop);

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Disable loop' })).not.toBeInTheDocument(),
    );
  });

  it('disables the loop from the band close button', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.loopRange = { enabled: true, startTick: 0, endTick: 3840 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const closeButton = await screen.findByRole('button', { name: 'Disable loop' });
    fireEvent.click(closeButton);

    await waitFor(() => expect(api.calls).toContain('updateTimelineLoopRange'));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Disable loop' })).not.toBeInTheDocument(),
    );
  });

  it('clears the punch range from the band close button without a time selection', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.punchRange = { startTick: 0, endTick: 1920 };
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    expect(screen.queryByText(/Selection/)).not.toBeInTheDocument();
    const closeButton = await screen.findByRole('button', { name: 'Clear punch range' });
    fireEvent.click(closeButton);

    await waitFor(() => expect(api.calls).toContain('updateTimelinePunchRange'));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Clear punch range' })).not.toBeInTheDocument(),
    );
  });

  it('deletes the selected marker with the Delete key', async () => {
    const session = defaultSession();
    session.workspace = 'arrange';
    session.arrangement.markers.push({ id: 'marker:verse', name: 'Verse', tick: 0 });
    const api = new FakeNativeApi({ bootstrapState: { session } });
    render(<Harness api={api} initialSession={session} />);

    const marker = await screen.findByText('Verse');
    fireEvent.pointerDown(marker.closest('[data-marker-id]')!, { clientX: 0 });
    fireEvent.keyDown(window, { key: 'Delete' });

    await waitFor(() => expect(api.calls).toContain('removeMarker'));
    await waitFor(() => expect(screen.queryByText('Verse')).not.toBeInTheDocument());
  });
});
