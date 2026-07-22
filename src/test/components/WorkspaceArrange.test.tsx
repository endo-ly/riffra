// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { WorkspaceArrange } from '@/components';
import { defaultSession, toAssetId, type CreativeSession } from '@/lib/domain';
import { FakeNativeApi } from '@/native/native-api-fake';

afterEach(cleanup);

function Harness({ api }: { api: FakeNativeApi }) {
  const initial = defaultSession();
  initial.workspace = 'arrange';
  const [session, setSession] = useState<CreativeSession>(initial);
  const [selectedClipIds, setSelectedClipIds] = useState<string[]>([]);
  return (
    <WorkspaceArrange
      session={session}
      setSession={setSession}
      selectedClipIds={selectedClipIds}
      setSelectedClipIds={setSelectedClipIds}
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
    await waitFor(() => expect(api.calls).toContain('pasteAudioClips'));
    expect(await screen.findByText('Take copy')).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'c', ctrlKey: true });
    fireEvent.keyDown(window, { key: 'v', ctrlKey: true });
    await waitFor(() => expect(api.calls).toContain('pasteAudioClips'));
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
});
