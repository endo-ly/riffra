// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AudioDriverPicker, CaptureSettings } from '@/components';
import { defaultSession, type AudioDeviceProbe } from '@/lib/domain';
import { FakeNativeApi } from '@/native/native-api-fake';

const probe: AudioDeviceProbe = {
  drivers: [
    {
      name: 'Windows Audio',
      accessMode: 'shared',
      inputs: ['Mic'],
      outputs: ['Speakers'],
    },
    {
      name: 'ASIO',
      accessMode: 'driverManaged',
      inputs: ['Input 1'],
      outputs: ['Output 1'],
    },
  ],
  midiInputs: [],
  midiOutputs: [],
  refreshedAtMs: 1,
  message: 'Audio device list refreshed.',
};

afterEach(cleanup);

describe('AudioDriverPicker', () => {
  it('shows the effective device values instead of pretending a preset value is active', () => {
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={() => undefined}
      />,
    );

    expect(screen.getByRole('button', { name: '48,000 Hz' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByRole('button', { name: '480 samples' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(screen.getByRole('button', { name: '64 samples' })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
  });

  it('requests a new buffer with the current driver and effective sample rate', async () => {
    const onSelect = vi.fn();
    const user = userEvent.setup();
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={onSelect}
      />,
    );

    await user.click(screen.getByRole('button', { name: '64 samples' }));

    expect(onSelect).toHaveBeenCalledWith('Windows Audio', 'Mic', 'Speakers', 48_000, 64);
  });

  it('switches drivers without replacing the effective format with a display default', async () => {
    const onSelect = vi.fn();
    const user = userEvent.setup();
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={onSelect}
      />,
    );

    await user.click(screen.getByRole('button', { name: /ASIO/ }));

    expect(onSelect).toHaveBeenCalledWith('ASIO', 'Input 1', 'Output 1', 48_000, 480);
  });

  it('shows whether the selected backend shares the Windows audio device', () => {
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={() => undefined}
      />,
    );

    expect(screen.getByText('Shared with other Windows applications')).toBeVisible();
  });
});

describe('CaptureSettings', () => {
  it('stores visual count-in changes in the Scratch Session', async () => {
    const session = defaultSession();
    const setSession = vi.fn();
    const api = new FakeNativeApi({ bootstrapState: { session } });
    const user = userEvent.setup();
    render(<CaptureSettings session={session} setSession={setSession} api={api} />);

    await user.selectOptions(screen.getByRole('combobox', { name: 'Visual count-in' }), '4');

    await waitFor(() =>
      expect(setSession).toHaveBeenCalledWith(
        expect.objectContaining({
          settings: expect.objectContaining({ countInBeats: 4 }),
        }),
      ),
    );
  });
});
