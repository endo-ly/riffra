// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AudioSettingsDialog } from '@/components';
import { fakeAudioStatus } from '@/native/native-api-fake';
import type { AudioDeviceProbe, AudioDriverConfig, AudioStatus } from '@/lib/domain';

const probe: AudioDeviceProbe = {
  drivers: [
    {
      name: 'Windows Audio',
      accessMode: 'shared',
      devicePairing: 'independent',
      inputs: [
        {
          name: 'Mic',
          channels: [
            { index: 0, name: 'Mic 1' },
            { index: 1, name: 'Mic 2' },
          ],
        },
      ],
      outputs: [{ name: 'Speakers', channels: [{ index: 0, name: 'Left' }] }],
    },
    {
      name: 'ASIO',
      accessMode: 'driverManaged',
      devicePairing: 'sameDevice',
      inputs: [
        {
          name: 'Focusrite USB ASIO',
          channels: [
            { index: 0, name: 'Input 1' },
            { index: 1, name: 'Input 2' },
          ],
        },
      ],
      outputs: [{ name: 'Focusrite USB ASIO', channels: [{ index: 0, name: 'Output 1' }] }],
    },
  ],
  midiInputs: [],
  midiOutputs: [],
  refreshedAtMs: 1,
  message: 'Audio device list refreshed.',
};

afterEach(cleanup);

function renderDialog(
  overrides: Partial<{
    audio: AudioStatus;
    probe: AudioDeviceProbe;
    onApply: (config: AudioDriverConfig) => Promise<AudioStatus>;
  }> = {},
) {
  const onClose = vi.fn();
  const onApply = overrides.onApply ?? vi.fn(async () => fakeAudioStatus());
  const activeProbe = overrides.probe ?? probe;
  render(
    <AudioSettingsDialog
      open
      audio={
        overrides.audio ??
        fakeAudioStatus({
          driver: 'Windows Audio',
          inputDevice: 'Mic',
          inputChannel: 0,
          outputDevice: 'Speakers',
        })
      }
      probe={activeProbe}
      safeMode={false}
      recordingActive={false}
      onClose={onClose}
      onRefresh={async () => activeProbe}
      onApply={onApply}
      onRecover={async () => fakeAudioStatus()}
    />,
  );
  return { onApply, onClose };
}

describe('AudioSettingsDialog', () => {
  it('does not apply settings when it is opened or cancelled', async () => {
    const onApply = vi.fn(async () => fakeAudioStatus());
    const { onClose } = renderDialog({ onApply });
    const user = userEvent.setup();

    expect(onApply).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(onApply).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('shows channel choices from the selected input device before applying', async () => {
    renderDialog();
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Input channel' }), '1');

    expect(screen.getByRole('option', { name: 'Mic 2' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Input channel' })).toHaveValue('1');
  });

  it('uses one Audio device selector for same-device drivers', async () => {
    renderDialog();
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Audio driver' }), 'ASIO');

    expect(screen.getByRole('combobox', { name: 'Audio device' })).toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Input device' })).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'Output device' })).not.toBeInTheDocument();
  });

  it('keeps the driver selector available when the active driver has no devices', async () => {
    const emptyDriverProbe: AudioDeviceProbe = {
      ...probe,
      drivers: [
        {
          name: 'Unavailable Driver',
          accessMode: 'driverManaged',
          devicePairing: 'independent',
          inputs: [],
          outputs: [],
        },
        ...probe.drivers,
      ],
    };
    renderDialog({
      probe: emptyDriverProbe,
      audio: fakeAudioStatus({ driver: 'Unavailable Driver' }),
    });
    const user = userEvent.setup();

    expect(screen.getByRole('combobox', { name: 'Audio driver' })).toBeInTheDocument();
    expect(screen.getByText('No audio devices are available for this driver.')).toBeInTheDocument();

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Audio driver' }),
      'Windows Audio',
    );

    expect(screen.getByRole('combobox', { name: 'Input device' })).toBeInTheDocument();
  });

  it('applies once and closes only after a successful response', async () => {
    const onApply = vi.fn(async () => fakeAudioStatus());
    const { onClose } = renderDialog({ onApply });
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Sample rate' }), '96000');
    await user.click(screen.getByRole('button', { name: 'Apply' }));

    expect(onApply).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('keeps the dialog open and reports a faulted response', async () => {
    const onApply = vi.fn(async () =>
      fakeAudioStatus({ state: 'faulted', message: 'The selected device could not be opened.' }),
    );
    const { onClose } = renderDialog({ onApply });
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Buffer size' }), '64');
    await user.click(screen.getByRole('button', { name: 'Apply' }));

    expect(onApply).toHaveBeenCalledOnce();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('alert')).toHaveTextContent('could not be opened');
  });

  it('disables Apply while recording is active', async () => {
    const onApply = vi.fn(async () => fakeAudioStatus());
    render(
      <AudioSettingsDialog
        open
        audio={fakeAudioStatus({
          driver: 'Windows Audio',
          inputDevice: 'Mic',
          outputDevice: 'Speakers',
        })}
        probe={probe}
        safeMode={false}
        recordingActive
        onClose={vi.fn()}
        onRefresh={async () => probe}
        onApply={onApply}
        onRecover={async () => fakeAudioStatus()}
      />,
    );
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Buffer size' }), '64');

    expect(screen.getByRole('button', { name: 'Apply' })).toBeDisabled();
    expect(onApply).not.toHaveBeenCalled();
  });
});
