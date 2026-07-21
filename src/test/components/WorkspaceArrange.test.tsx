// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { WorkspaceArrange } from '@/components';
import { defaultSession, toAssetId, type CreativeSession } from '@/lib/domain';
import { FakeNativeApi } from '@/native/native-api-fake';

afterEach(cleanup);

function Harness({ api }: { api: FakeNativeApi }) {
  const initial = defaultSession();
  initial.workspace = 'arrange';
  const [session, setSession] = useState<CreativeSession>(initial);
  return <WorkspaceArrange session={session} setSession={setSession} api={api} />;
}

describe('WorkspaceArrange', () => {
  it('creates the first audio track when an audio Asset is dropped on an empty timeline', async () => {
    const api = new FakeNativeApi({ bootstrapState: { session: defaultSession() } });
    render(<Harness api={api} />);
    const empty = screen.getByText('Drop audio here').parentElement!;
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
    expect(await screen.findByText('Take')).toBeInTheDocument();
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
});
