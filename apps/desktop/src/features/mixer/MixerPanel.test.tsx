import { TestSessionProvider } from '@/test/session-provider';
// @vitest-environment jsdom
import type { CreativeSession, EffectOwner, MixBus } from '@/model/generated';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import { MixerPanel } from './MixerPanel';

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
  const api = new FakeNativeApi();
  const receipt = () => ({
    canonical: canonicalState(structuredClone(session)),
    createdEntityIds: {},
    projection: { state: 'notRequired' as const },
  });
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
  const open = vi.fn();
  const openAutomation = vi.fn();
  function Harness() {
    const [current, setCurrent] = useState<CreativeSession>(structuredClone(session));
    const [owner, setOwner] = useState<EffectOwner | null>(null);
    return (
      <TestSessionProvider api={api} applyCanonicalState={(state) => setCurrent(state.session)}>
        <MixerPanel
          session={current}
          selectedTrackId={null}
          selectedOwner={owner}
          onSelectOwner={setOwner}
          onOpenOwnerDevices={open}
          onSelectTrack={() => undefined}
          onOpenDevices={() => undefined}
          onOpenAutomation={openAutomation}
          missingDeviceIds={[]}
        />
      </TestSessionProvider>
    );
  }
  render(<Harness />);
  const add = async (name: string, kind: MixBus['kind']) => {
    fireEvent.change(screen.getByLabelText('New channel name'), { target: { value: name } });
    fireEvent.change(screen.getByLabelText('New channel kind'), { target: { value: kind } });
    fireEvent.click(screen.getByRole('button', { name: 'Add channel' }));
    await screen.findByRole('article', { name: `${name} mixer channel` });
  };
  await add('Group', 'group');
  await add('Reverb', 'return');
  fireEvent.click(screen.getByRole('button', { name: 'Master · 0 Sends' }));
  fireEvent.change(screen.getByLabelText('Main output'), { target: { value: 'bus:group' } });
  await waitFor(() => expect(screen.getByLabelText('Main output')).toHaveValue('bus:group'));
  fireEvent.change(screen.getByLabelText('Add Send'), { target: { value: 'bus:return' } });
  const send = await screen.findByRole('group', { name: 'Reverb send' });
  const slider = within(send).getByLabelText('Reverb send level');
  fireEvent.click(within(send).getByRole('button', { name: 'Automation' }));
  expect(openAutomation).toHaveBeenLastCalledWith({ kind: 'sendLevel', sendId: 'send:1' });
  const enabled = within(send).getByRole('switch', { name: 'Enabled' });
  fireEvent.click(enabled);
  await waitFor(() => expect(session.arrangement.mixer.sends[0].enabled).toBe(false));
  expect(api.updateSend).toHaveBeenCalledWith('send:1', { enabled: false });
  fireEvent.change(slider, { target: { value: '-12' } });
  expect(api.updateSend).toHaveBeenCalledTimes(1);
  fireEvent.pointerUp(slider);
  fireEvent.blur(slider);
  await waitFor(() => expect(session.arrangement.mixer.sends[0].levelDb).toBe(-12));
  expect(api.updateSend).toHaveBeenCalledTimes(2);
  expect(api.previewSendLevel).not.toHaveBeenCalled();
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
  expect(openAutomation).toHaveBeenLastCalledWith({
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
  expect(open).toHaveBeenLastCalledWith({ kind: 'bus', id: 'bus:group' });
  fireEvent.click(
    within(screen.getByRole('complementary', { name: 'Master mixer channel' })).getByRole(
      'button',
      { name: 'FX 0' },
    ),
  );
  expect(open).toHaveBeenLastCalledWith({ kind: 'master' });
  fireEvent.click(
    within(screen.getByRole('complementary', { name: 'Master mixer channel' })).getByRole(
      'button',
      { name: 'Automation' },
    ),
  );
  expect(openAutomation).toHaveBeenLastCalledWith({ kind: 'masterGain' });
});
