// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AudioDriverPicker } from '@/components';
import type { AudioDeviceProbe } from '@/lib/domain';

const probe: AudioDeviceProbe = {
  drivers: [
    {
      name: 'Windows Audio',
      accessMode: 'shared',
      devicePairing: 'independent',
      inputs: ['Mic'],
      outputs: ['Speakers'],
    },
    {
      name: 'ASIO',
      accessMode: 'driverManaged',
      devicePairing: 'sameDevice',
      inputs: ['Focusrite USB ASIO'],
      outputs: ['Focusrite USB ASIO'],
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
        inputChannel={0}
        inputChannels={[{ index: 0, name: 'Mic 1' }]}
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
        inputChannel={0}
        inputChannels={[{ index: 0, name: 'Mic 1' }]}
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={onSelect}
      />,
    );

    await user.click(screen.getByRole('button', { name: '64 samples' }));

    expect(onSelect).toHaveBeenCalledWith('Windows Audio', 'Mic', 0, 'Speakers', 48_000, 64);
  });

  it('switches drivers without replacing the effective format with a display default', async () => {
    const onSelect = vi.fn();
    const user = userEvent.setup();
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        inputChannel={0}
        inputChannels={[{ index: 0, name: 'Mic 1' }]}
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={onSelect}
      />,
    );

    await user.click(screen.getByRole('button', { name: /ASIO/ }));

    expect(onSelect).toHaveBeenCalledWith(
      'ASIO',
      'Focusrite USB ASIO',
      0,
      'Focusrite USB ASIO',
      48_000,
      480,
    );
  });

  it('shows whether the selected backend shares the Windows audio device', () => {
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        inputChannel={0}
        inputChannels={[{ index: 0, name: 'Mic 1' }]}
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={() => undefined}
      />,
    );

    expect(screen.getByText('Shared with other Windows applications')).toBeVisible();
  });

  it('routes the selected physical input channel', async () => {
    const onSelect = vi.fn();
    const user = userEvent.setup();
    render(
      <AudioDriverPicker
        probe={probe}
        current="Windows Audio"
        inputDevice="Mic"
        inputChannel={0}
        inputChannels={[
          { index: 0, name: 'Analogue 1' },
          { index: 1, name: 'Analogue 2' },
        ]}
        outputDevice="Speakers"
        sampleRate={48_000}
        bufferSize={480}
        onSelect={onSelect}
      />,
    );

    await user.selectOptions(screen.getByRole('combobox', { name: 'Input channel' }), '1');

    expect(onSelect).toHaveBeenCalledWith('Windows Audio', 'Mic', 1, 'Speakers', 48_000, 480);
  });
});
