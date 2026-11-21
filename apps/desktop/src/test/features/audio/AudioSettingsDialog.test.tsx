// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AudioSettingsDialog } from '@/features/audio/AudioSettingsDialog';
import { createFakeNativeApi, fakeAudioStatus } from '@/test/support/fake-native-api';
import { createTestSessionStore, renderWithSession } from '@/test/support/session-provider';
import type { AudioDeviceProbe, AudioStatus } from '@/model/generated';

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
  refreshedAtMs: 1,
  message: 'Audio device list refreshed.',
};

afterEach(cleanup);

const defaultStatus = () =>
  fakeAudioStatus({
    driver: 'Windows Audio',
    inputDevice: 'Mic',
    inputChannel: 0,
    inputChannels: [
      { index: 0, name: 'Mic 1' },
      { index: 1, name: 'Mic 2' },
    ],
    outputDevice: 'Speakers',
    outputChannels: [{ index: 0, name: 'Left' }],
  });

async function renderDialog({
  status = defaultStatus(),
  probe: activeProbe = probe,
}: {
  status?: AudioStatus;
  probe?: AudioDeviceProbe;
} = {}) {
  const api = createFakeNativeApi({
    audio: status,
    responses: { probeAudioDevices: activeProbe },
  });
  const onClose = vi.fn();
  const store = createTestSessionStore(api);
  await store.getState().audio.refreshDevices();
  renderWithSession(<AudioSettingsDialog onClose={onClose} />, { api, store });
  return { api, store, onClose };
}

describe('AudioSettingsDialog', () => {
  it('does not apply settings when it is opened or cancelled', async () => {
    const { api, onClose } = await renderDialog();
    const setAudioDriver = vi.spyOn(api, 'setAudioDriver');
    const user = userEvent.setup();

    expect(setAudioDriver).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(setAudioDriver).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('shows channel choices from the selected input device before applying', async () => {
    await renderDialog();
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Input channel' }), '1');

    expect(screen.getByRole('option', { name: 'Mic 2' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Input channel' })).toHaveValue('1');
  });

  it('runs one device detail-probe lifecycle without retrying stale results', async () => {
    // Arrange: a probe that reports devices without channel details.
    const passiveProbe: AudioDeviceProbe = {
      ...probe,
      drivers: probe.drivers.map((driver) => ({
        ...driver,
        inputs: driver.inputs.map((device) => ({ ...device, channels: [] })),
        outputs: driver.outputs.map((device) => ({ ...device, channels: [] })),
      })),
    };
    const audio = fakeAudioStatus({
      driver: 'Windows Audio',
      inputDevice: 'Mic',
      inputChannel: 1,
      inputChannels: [
        { index: 0, name: 'Mic 1' },
        { index: 1, name: 'Mic 2' },
      ],
      outputDevice: 'Speakers',
      outputChannels: [{ index: 0, name: 'Left' }],
    });
    const { api, store } = await renderDialog({ probe: passiveProbe, status: audio });
    const probeChannels = vi
      .spyOn(api, 'probeDeviceChannels')
      .mockResolvedValueOnce({
        driver: 'ASIO',
        inputDevice: 'Focusrite USB ASIO',
        inputChannels: [],
        outputDevice: 'Focusrite USB ASIO',
        outputChannels: [],
      })
      .mockRejectedValueOnce(new Error('probe failed'))
      .mockResolvedValue({
        driver: 'ASIO',
        inputDevice: 'Focusrite USB ASIO',
        inputChannels: [
          { index: 0, name: 'Input 1' },
          { index: 1, name: 'Input 2' },
        ],
        outputDevice: 'Focusrite USB ASIO',
        outputChannels: [{ index: 0, name: 'Output 1' }],
      });

    await waitFor(() => expect(screen.getByRole('option', { name: 'Mic 2' })).toBeInTheDocument());
    expect(probeChannels).not.toHaveBeenCalled();
    const user = userEvent.setup();

    // Act: selecting a driver lazily probes its interface once.
    await user.selectOptions(screen.getByRole('combobox', { name: 'Audio driver' }), 'ASIO');
    await waitFor(() => expect(probeChannels).toHaveBeenCalledOnce());
    await new Promise((resolve) => window.setTimeout(resolve, 50));

    // Assert: a refresh does not repeat the finished probe and reports its failure.
    expect(probeChannels).toHaveBeenCalledOnce();
    api.setResponse('probeAudioDevices', { ...passiveProbe, refreshedAtMs: 2 });
    await act(() => store.getState().audio.refreshDevices());
    await waitFor(() => expect(probeChannels).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('probe failed'));
    await new Promise((resolve) => window.setTimeout(resolve, 50));
    expect(probeChannels).toHaveBeenCalledTimes(2);

    // Act: a later refresh probes the still-unknown interface again.
    api.setResponse('probeAudioDevices', { ...passiveProbe, refreshedAtMs: 3 });
    await act(() => store.getState().audio.refreshDevices());

    // Assert: the probe result fills the channel choices.
    await waitFor(() => expect(probeChannels).toHaveBeenCalledTimes(3));
    expect(screen.getByRole('option', { name: 'Input 2' })).toBeInTheDocument();
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
    await renderDialog({
      probe: emptyDriverProbe,
      status: fakeAudioStatus({ driver: 'Unavailable Driver' }),
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

  it('applies once, then keeps the dialog open for a faulted response', async () => {
    const { api, onClose } = await renderDialog();
    const setAudioDriver = vi
      .spyOn(api, 'setAudioDriver')
      .mockResolvedValueOnce(fakeAudioStatus())
      .mockResolvedValueOnce(
        fakeAudioStatus({ state: 'faulted', message: 'The selected device could not be opened.' }),
      );
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Sample rate' }), '96000');
    await user.click(screen.getByRole('button', { name: 'Apply' }));

    expect(setAudioDriver).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
    await user.selectOptions(screen.getByRole('combobox', { name: 'Buffer size' }), '64');
    await user.click(screen.getByRole('button', { name: 'Apply' }));

    expect(setAudioDriver).toHaveBeenCalledTimes(2);
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.getByRole('alert')).toHaveTextContent('could not be opened');
  });

  it('disables Apply while recording is active', async () => {
    const status = defaultStatus();
    status.recording = { ...status.recording, active: true };
    const { api } = await renderDialog({ status });
    const setAudioDriver = vi.spyOn(api, 'setAudioDriver');
    const user = userEvent.setup();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Buffer size' }), '64');

    expect(screen.getByRole('button', { name: 'Apply' })).toBeDisabled();
    expect(setAudioDriver).not.toHaveBeenCalled();
  });
});
