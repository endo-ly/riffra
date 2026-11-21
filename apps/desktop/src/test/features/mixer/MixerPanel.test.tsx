// @vitest-environment jsdom
import type { MixBus } from '@/model/generated';
import { defaultSession } from '@/native/preview/fixtures';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { MixerPanel } from '@/features/mixer/MixerPanel';
import { ToastStack } from '@/shared/ui/message/ToastStack';

afterEach(cleanup);

it('edits Group and Send routes, commits their controls, and opens Bus and Master Devices', async () => {
  const session = defaultSession();
  session.arrangement.tracks.push({
    id: 'track:1',
    name: 'Keys',
    kind: 'instrument',
    gainDb: 0,
    pan: 0,
    externalAudioSourceTrackIds: [],
    panLaw: 'equalPower',
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    effects: [],
  });
  const api = createFakeNativeApi();
  const receipt = () => api.mutationResult(structuredClone(session));
  api.addMixBus = vi.fn(async (name, kind) => {
    session.arrangement.mixer.buses.push({
      id: `bus:${kind}`,
      name,
      kind,
      gainDb: 0,
      pan: 0,
      panLaw: 'equalPower',
      muted: false,
      solo: false,
      effects: [],
    });
    return receipt();
  });
  api.setTrackOutput = vi.fn(async (_, busId) => {
    session.arrangement.tracks[0].outputBusId = busId ?? undefined;
    return receipt();
  });
  api.addSend = vi.fn(async (trackId, returnBusId, levelDb, position) => {
    session.arrangement.mixer.sends.push({
      id: 'send:1',
      trackId,
      returnBusId,
      levelDb,
      position,
      enabled: true,
    });
    return receipt();
  });
  api.updateSend = vi.fn(async (id, patch) => {
    if (patch.levelDb === -9) throw new Error('send rejected');
    Object.assign(
      session.arrangement.mixer.sends.find((send) => send.id === id)!,
      patch,
    );
    return receipt();
  });
  api.updateMixBus = vi.fn(async (id, patch) => {
    Object.assign(
      session.arrangement.mixer.buses.find((bus) => bus.id === id)!,
      patch,
    );
    return receipt();
  });
  api.removeMixBus = vi.fn(async (id) => {
    session.arrangement.mixer.buses = session.arrangement.mixer.buses.filter(
      (bus) => bus.id !== id,
    );
    return receipt();
  });
  api.previewChannelMix = vi.fn().mockResolvedValue(undefined);
  api.previewSendLevel = vi.fn().mockResolvedValue(undefined);
  // The store owns its snapshot; the stubs mutate the test's copy so accepted canonicals differ.
  const store = createTestSessionStore(api, { session: structuredClone(session) });
  render(
    <TestSessionProvider api={api} store={store}>
      <MixerPanel />
      <ToastStack />
    </TestSessionProvider>,
  );
  expect(screen.getByLabelText('Keys gain')).toHaveStyle('--fill-direction: to top');
  expect(screen.getByLabelText('Keys pan')).toHaveStyle('--fill-direction: to right');
  expect(screen.getByLabelText('Master mixer gain')).toHaveStyle('--fill-direction: to top');
  const add = async (name: string, kind: MixBus['kind']) => {
    fireEvent.change(screen.getByLabelText('New channel name'), { target: { value: name } });
    fireEvent.change(screen.getByLabelText('New channel kind'), { target: { value: kind } });
    fireEvent.click(screen.getByRole('button', { name: 'Add channel' }));
    await screen.findByRole('article', { name: `${name} mixer channel` });
  };
  await add('Group', 'group');
  expect(screen.getByLabelText('Group gain')).toHaveStyle('--fill-direction: to top');
  await add('Reverb', 'return');
  fireEvent.click(screen.getByRole('button', { name: 'Master · 0 Sends' }));
  fireEvent.change(screen.getByLabelText('Main output'), { target: { value: 'bus:group' } });
  await waitFor(() => expect(screen.getByLabelText('Main output')).toHaveValue('bus:group'));
  fireEvent.change(screen.getByLabelText('Add Send'), { target: { value: 'bus:return' } });
  const send = await screen.findByRole('group', { name: 'Reverb send' });
  const slider = within(send).getByLabelText('Reverb send level');
  fireEvent.click(within(send).getByRole('button', { name: 'Automation' }));
  expect(store.getState().workspace.automation.trackTargets['track:1']).toEqual({
    kind: 'sendLevel',
    sendId: 'send:1',
  });
  fireEvent.pointerDown(slider);
  fireEvent.change(slider, { target: { value: '-12' } });
  fireEvent.pointerUp(slider);
  fireEvent.blur(slider);
  await waitFor(() => expect(session.arrangement.mixer.sends[0].levelDb).toBe(-12));
  expect(api.updateSend).toHaveBeenCalledTimes(1);
  expect(api.previewSendLevel).toHaveBeenCalledWith('send:1', -12);
  fireEvent.pointerDown(slider);
  fireEvent.change(slider, { target: { value: '-9' } });
  fireEvent.pointerUp(slider);
  expect(await screen.findByText('send rejected')).toBeInTheDocument();
  const routingDialog = screen.getByRole('dialog', { name: 'Keys routing' });
  const errorText = screen.getByText('send rejected');
  const notification = errorText.closest('[role="alert"]');
  expect(notification).toContainElement(errorText);
  expect(routingDialog).not.toContainElement(errorText);
  expect(session.arrangement.mixer.sends[0].levelDb).toBe(-12);
  const enabled = within(send).getByRole('switch', { name: 'Enabled' });
  fireEvent.click(enabled);
  await waitFor(() => expect(session.arrangement.mixer.sends[0].enabled).toBe(false));
  expect(api.updateSend).toHaveBeenCalledWith('send:1', { enabled: false });
  expect(slider).toBeDisabled();
  fireEvent.click(enabled);
  await waitFor(() => expect(session.arrangement.mixer.sends[0].enabled).toBe(true));
  expect(api.updateSend).toHaveBeenCalledWith('send:1', { enabled: true });
  expect(session.arrangement.mixer.sends[0].levelDb).toBe(-12);
  fireEvent.click(within(send).getByRole('button', { name: 'Pre' }));
  await waitFor(() =>
    expect(within(send).getByRole('button', { name: 'Pre' })).toHaveAttribute(
      'aria-pressed',
      'true',
    ),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Close' }));
  expect(await screen.findByRole('button', { name: 'Group · 1 Sends' })).toBeInTheDocument();
  const group = screen.getByRole('article', { name: 'Group mixer channel' });
  fireEvent.click(within(group).getByRole('button', { name: 'Automation' }));
  expect(store.getState().workspace.automation.mixerTarget).toEqual({
    kind: 'busMix',
    busId: 'bus:group',
    parameter: 'gain',
  });
  const gain = within(group).getByLabelText('Group gain');
  fireEvent.pointerDown(gain);
  fireEvent.change(gain, { target: { value: '-9' } });
  fireEvent.pointerUp(gain);
  fireEvent.blur(gain);
  await waitFor(() => expect(session.arrangement.mixer.buses[0].gainDb).toBe(-9));
  fireEvent.click(within(group).getByRole('button', { name: 'Group mute' }));
  await waitFor(() =>
    expect(within(group).getByRole('button', { name: 'Group mute' })).toHaveAttribute(
      'aria-pressed',
      'true',
    ),
  );
  fireEvent.click(within(group).getByRole('button', { name: 'FX 0' }));
  expect(store.getState().workspace.selection).toEqual({ kind: 'bus', busId: 'bus:group' });
  expect(store.getState().workspace.lower.view).toBe('devices');
  fireEvent.click(
    within(screen.getByRole('complementary', { name: 'Master mixer channel' })).getByRole(
      'button',
      { name: 'FX 0' },
    ),
  );
  expect(store.getState().workspace.selection).toEqual({ kind: 'master' });
  fireEvent.click(
    within(screen.getByRole('complementary', { name: 'Master mixer channel' })).getByRole(
      'button',
      { name: 'Automation' },
    ),
  );
  expect(store.getState().workspace.automation.mixerTarget).toEqual({ kind: 'masterGain' });
});
