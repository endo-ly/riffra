import { TestSessionProvider } from '@/test/session-provider';
// @vitest-environment jsdom
import type { RenderResult } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { toAssetId } from '@/model/asset-id';
import { FakeNativeApi } from '@/native/native-api-fake';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import { MixdownDialog } from './MixdownDialog';

afterEach(cleanup);

it('starts an unchanged mixdown render without committing a settings mutation', async () => {
  const session = defaultSession();
  const api = new FakeNativeApi();
  const setMixdown = vi.fn(async () => {
    throw new Error('unchanged settings must not be committed');
  });
  api.setMixdown = setMixdown;
  const startRenderJob = vi.fn(async () => ({
    kind: 'render' as const,
    id: 'job:render',
    state: 'running' as const,
    progress: 0,
    message: 'rendering',
    result: null,
  }));
  api.startRenderJob = startRenderJob;
  api.getBackgroundJob = vi.fn(async () => ({
    kind: 'render' as const,
    id: 'job:render',
    state: 'running' as const,
    progress: 0,
    message: 'rendering',
    result: null,
  }));
  render(
    <TestSessionProvider api={api}>
      <MixdownDialog session={session} onClose={() => undefined} />
    </TestSessionProvider>,
  );

  fireEvent.click(screen.getByRole('button', { name: 'Save and render' }));

  await waitFor(() => expect(startRenderJob).toHaveBeenCalledOnce());
  expect(setMixdown).not.toHaveBeenCalled();
});

it.each(['measured', 'unavailable'] as const)(
  'saves canonical mastering targets and displays the completed %s WAV',
  async (state) => {
    const session = defaultSession();
    const api = new FakeNativeApi();
    const setMixdown = vi.fn(async (settings) => {
      session.settings.mixdown = structuredClone(settings);
      return {
        canonical: canonicalState(structuredClone(session)),
        createdEntityIds: {},
        projection: { state: 'notRequired' as const },
      };
    });
    api.setMixdown = setMixdown;
    const result: RenderResult = {
      mastering:
        state === 'measured'
          ? {
              target: { integratedLufs: -16, truePeakDb: -2, loudnessRangeLu: 11 },
              input: { integratedLufs: -21, truePeakDb: -7, loudnessRangeLu: 4 },
              deviation: { integratedLufs: -0.12, truePeakDb: -0.08, loudnessRangeLu: -7 },
              normalizationType: 'limited',
              truePeakCorrectionDb: 0.08,
            }
          : null,
      measurements: {
        sections: [],
        sectionLoudnessState:
          state === 'measured'
            ? { state: 'measured' }
            : { state: 'unavailable', reason: 'section log is invalid' },
        samplePeakDbfs: state === 'measured' ? -3.01 : 1.94,
        loudness:
          state === 'measured'
            ? {
                state,
                measurement: { integratedLufs: -16.12, truePeakDb: -2.08, loudnessRangeLu: 4 },
              }
            : { state, reason: 'ffmpeg is unavailable' },
      },
      warnings: state === 'measured' ? [] : ['sample peak exceeds 0 dbfs'],
      assetId: toAssetId('asset:output'),
      projectId: null,
      createdAtMs: null,
      path: 'C:/renders/final.wav',
      exportedPath: null,
      sampleRate: 48000,
      frames: 192000,
      durationMs: 4000,
      clipCount: 1,
      rangeStartMs: 0,
      rangeEndMs: 4000,
      normalized: false,
      trackId: null,
      state: 'completed',
      message: 'completed',
    };
    const startRenderJob = vi.fn(async () => ({
      kind: 'render' as const,
      id: 'job:render',
      state: 'running' as const,
      progress: 0,
      message: 'rendering',
      result: null,
    }));
    api.startRenderJob = startRenderJob;
    api.getBackgroundJob = vi.fn(async () => ({
      kind: 'render' as const,
      id: 'job:render',
      state: 'completed' as const,
      progress: 1,
      message: 'completed',
      result,
    }));
    function Harness() {
      const [current, setCurrent] = useState(structuredClone(session));
      return (
        <TestSessionProvider
          api={api}
          applyCanonicalState={(canonical) => setCurrent(canonical.session)}
        >
          <MixdownDialog session={current} onClose={() => undefined} />
        </TestSessionProvider>
      );
    }
    render(<Harness />);
    fireEvent.change(screen.getByLabelText('Tail (seconds)'), { target: { value: '3' } });
    fireEvent.change(screen.getByLabelText('Plugin preparation time (seconds)'), {
      target: { value: '10' },
    });
    if (state === 'measured') {
      fireEvent.click(screen.getByLabelText('Loudness Mastering'));
      fireEvent.change(screen.getByLabelText('Target Integrated LUFS'), {
        target: { value: '-16' },
      });
      fireEvent.change(screen.getByLabelText('True Peak ceiling (dBTP)'), {
        target: { value: '-2' },
      });
    }
    fireEvent.click(screen.getByRole('button', { name: 'Save and render' }));
    await waitFor(() => expect(session.settings.mixdown.tailSeconds).toBe(3));
    expect(session.settings.mixdown.pluginWarmUpSeconds).toBe(10);
    if (state === 'measured') {
      expect(session.settings.mixdown.mastering?.integratedLufs).toBe(-16);
      expect(session.settings.mixdown.mastering?.truePeakDb).toBe(-2);
    } else expect(session.settings.mixdown.mastering).toBeNull();
    await screen.findByText('C:/renders/final.wav');
    expect(setMixdown).toHaveBeenCalledOnce();
    expect(startRenderJob).toHaveBeenCalledOnce();
    expect(setMixdown.mock.invocationCallOrder[0]).toBeLessThan(
      startRenderJob.mock.invocationCallOrder[0],
    );
    if (state === 'measured') {
      expect(screen.getByText('-16.12 LUFS')).toBeInTheDocument();
      expect(screen.getByText('-2.08 dBTP')).toBeInTheDocument();
      expect(screen.getByText('4.00 LU')).toBeInTheDocument();
      expect(screen.getByText('-3.01 dBFS')).toBeInTheDocument();
      expect(screen.getByText(/True Peak output gain correction/)).toHaveTextContent('0.08 dB');
    } else {
      expect(screen.getByText('ffmpeg is unavailable')).toBeInTheDocument();
      expect(screen.getByText('section log is invalid')).toBeInTheDocument();
      expect(screen.getByText('1.94 dBFS')).toBeInTheDocument();
      expect(screen.getByRole('alert')).toHaveTextContent('sample peak exceeds 0 dbfs');
    }
  },
);
