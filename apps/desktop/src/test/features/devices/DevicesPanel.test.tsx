import type * as SidechainModule from '@/features/devices/DeviceSidechainEditor';
import type * as InlineMessageModule from '@/shared/ui/message/InlineMessage';
import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';
// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { Profiler, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  CreativeSession,
  DeviceSidechainCapability,
  EffectOwner,
  DeviceParameterInfo,
  InstrumentLibraryItem,
  Track,
} from '@/model/generated';
import { defaultSession } from '@/native/preview/fixtures';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { ownerSelection } from '@/model/editor-selection';
import { resolveDeviceTarget } from '@/features/devices/model/device-target';
import { DevicesPanel } from '@/features/devices/DevicesPanel';
import { useDeviceDetails } from '@/features/devices/hooks/useDeviceDetails';
import { HostConnectionChangedError } from '@/native/invoke';

const sidechainRenders = vi.hoisted(() => ({ count: 0 }));
vi.mock('@/features/devices/DeviceSidechainEditor', async (importOriginal) => {
  const actual = await importOriginal<typeof SidechainModule>();
  return {
    ...actual,
    DeviceSidechainEditor: (props: Parameters<typeof actual.DeviceSidechainEditor>[0]) => {
      sidechainRenders.count += 1;
      return <actual.DeviceSidechainEditor {...props} />;
    },
  };
});
const inlineMessageRenders = vi.hoisted(() => ({ count: 0 }));
vi.mock('@/shared/ui/message/InlineMessage', async (importOriginal) => {
  const actual = await importOriginal<typeof InlineMessageModule>();
  return {
    ...actual,
    InlineMessage: (props: Parameters<typeof actual.InlineMessage>[0]) => {
      inlineMessageRenders.count += 1;
      return <actual.InlineMessage {...props} />;
    },
  };
});

afterEach(cleanup);
const effect = {
  id: 'effect:1',
  name: 'Test Effect',
  bypassed: false,
  plugin: { path: 'test.vst3', parameterValues: [0.5, 0], disabledPlaceholder: false },
};
const track: Track = {
  externalAudioSourceTrackIds: [],
  panLaw: 'equalPower' as const,
  id: 'track:1',
  name: 'Audio',
  kind: 'audio',
  gainDb: 0,
  pan: 0,
  muted: false,
  solo: false,
  armed: false,
  monitoring: 'off',
  midiInput: {},
  effects: [effect],
};
const parameters: DeviceParameterInfo[] = [
  {
    index: 0,
    parameterId: 'level',
    name: 'Level',
    value: 0.5,
    defaultValue: 0.25,
    displayValue: '-6',
    label: 'dB',
    automatable: true,
    discrete: false,
    stepCount: 0,
    choices: [],
  },
  {
    index: 1,
    parameterId: 'mode',
    name: 'Mode',
    value: 0,
    defaultValue: 0,
    displayValue: 'Clean',
    label: '',
    automatable: true,
    discrete: true,
    stepCount: 2,
    choices: [
      { value: 0, displayValue: 'Clean' },
      { value: 1, displayValue: 'Warm' },
    ],
  },
];
function runtimeApi(
  sidechain: DeviceSidechainCapability = {
    available: false,
    mono: false,
    stereo: false,
    channels: 0,
  },
  instruments: InstrumentLibraryItem[] = [],
) {
  return createFakeNativeApi({
    instruments,
    responses: {
      inspectDevice: {
        id: effect.id,
        name: effect.name,
        source: 'vst3',
        bypassed: false,
        capabilities: {
          parameters: true,
          presets: true,
          editor: true,
          state: true,
          sidechain,
        },
        parameterCount: 2,
        statePersisted: false,
      },
      listDeviceParameters: parameters,
      listPluginPresets: [
        { index: 0, name: 'Init' },
        { index: 1, name: 'Soft' },
      ],
      getPluginPreset: { index: 0, name: 'Init' },
    },
  });
}
function renderDevices({
  api,
  initialTrack = track,
  initialSession,
  owner = { kind: 'track', id: track.id },
  wrapper,
}: {
  api: FakeNativeApi;
  initialTrack?: Track;
  initialSession?: CreativeSession;
  owner?: EffectOwner;
  wrapper?: (content: ReactNode) => ReactNode;
}) {
  const initial = initialSession ?? defaultSession();
  if (!initialSession) initial.arrangement.tracks = [structuredClone(initialTrack)];
  const store = createTestSessionStore(api, { session: initial });
  store.getState().workspace.setSelection(ownerSelection(owner));
  void store.getState().catalog.reloadInstruments();
  const content = (
    <TestSessionProvider api={api} store={store}>
      <DevicesPanel />
    </TestSessionProvider>
  );
  const tree = wrapper ? wrapper(content) : content;
  const { rerender } = render(tree);
  return { store, rerender: () => rerender(tree) };
}

describe('DevicesPanel', () => {
  it.each(['metadata refresh', 'host change', 'device change', 'unmount'] as const)(
    'handles a pending edit across %s',
    async (change) => {
      const api = runtimeApi();
      const store = createTestSessionStore(api);
      const { result, rerender, unmount } = renderHook(
        ({ values, deviceId }) =>
          useDeviceDetails(
            { kind: 'track', id: track.id },
            deviceId,
            effect.plugin.path,
            values,
            undefined,
          ),
        {
          initialProps: { values: [0.5, 0], deviceId: effect.id },
          wrapper: ({ children }) => (
            <TestSessionProvider api={api} store={store}>
              {children}
            </TestSessionProvider>
          ),
        },
      );
      await waitFor(() => expect(result.current.loading).toBe(false));
      let resolve!: (value: ReturnType<typeof api.mutationResult>) => void;
      let reject!: (error: Error) => void;
      const pending = new Promise<ReturnType<typeof api.mutationResult>>((done, fail) => {
        resolve = done;
        reject = fail;
      });
      let saved!: Promise<boolean>;
      act(() => {
        saved = result.current.mutate(() => pending);
      });

      if (change === 'metadata refresh') {
        rerender({ values: [0.75, 0], deviceId: effect.id });
      } else if (change === 'device change') {
        rerender({ values: [0.5, 0], deviceId: 'effect:other' });
      } else if (change === 'unmount') {
        unmount();
      } else {
        act(() => {
          const state = store.getState();
          const boot = state.project.boot!;
          state.host.adoptConnection({
            ...state.host.connection,
            generation: state.host.connection.generation + 1,
          });
          store.getState().project.acceptBootstrap(boot);
        });
      }
      await waitFor(() => expect(result.current.loading).toBe(false));
      const previous = store.getState().project.boot!.canonical;
      const next = api.mutationResult({
        ...api.bootstrapState.canonical.session,
        projectName: 'Saved edit',
      });
      await act(async () => {
        if (change === 'host change') reject(new HostConnectionChangedError());
        else resolve(next);
      });

      expect(await saved).toBe(change === 'metadata refresh');
      expect(store.getState().project.boot!.canonical).toStrictEqual(
        change === 'host change' ? previous : next.canonical,
      );
      if (change !== 'unmount') await waitFor(() => expect(result.current.saving).toBe(false));
    },
  );

  it('does not share metadata with a replaced plugin that reuses the device ID', async () => {
    const api = runtimeApi();
    let resolveOld!: (inspection: Awaited<ReturnType<typeof api.inspectDevice>>) => void;
    const old = await api.inspectDevice({ kind: 'track', id: track.id }, effect.id);
    const inspect = vi
      .spyOn(api, 'inspectDevice')
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
      )
      .mockResolvedValue({ ...old, name: 'Replacement' });
    const { result, rerender } = renderHook(
      ({ pluginPath }) =>
        useDeviceDetails(
          { kind: 'track', id: track.id },
          effect.id,
          pluginPath,
          [0.5, 0],
          undefined,
        ),
      {
        initialProps: { pluginPath: 'old.vst3' },
        wrapper: ({ children }) => <TestSessionProvider api={api}>{children}</TestSessionProvider>,
      },
    );
    await waitFor(() => expect(inspect).toHaveBeenCalledTimes(1));

    rerender({ pluginPath: 'replacement.vst3' });
    await waitFor(() => expect(result.current.details?.inspection.name).toBe('Replacement'));
    await act(async () => resolveOld(old));

    expect(inspect).toHaveBeenCalledTimes(2);
    expect(result.current.details?.inspection.name).toBe('Replacement');
  });

  it('updates only the automated parameter when the transport position changes', async () => {
    const api = runtimeApi();
    vi.spyOn(api, 'formatDeviceParameterValue').mockImplementation(
      async (_owner, _device, _parameter, value) => String(value),
    );
    const session = defaultSession();
    session.arrangement.tracks = [structuredClone(track)];
    session.arrangement.automationLanes = [
      {
        id: 'lane',
        target: { kind: 'deviceParameter', deviceId: effect.id, parameterId: 'level' },
        interpolation: 'linear',
        points: [
          { id: 'start', tick: 0, value: 0.2 },
          { id: 'end', tick: 960, value: 0.8 },
        ],
      },
    ];
    const store = createTestSessionStore(api, { session });
    store.getState().workspace.setSelection({ kind: 'track', trackId: track.id });
    render(
      <TestSessionProvider api={api} store={store}>
        <DevicesPanel />
      </TestSessionProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: effect.name }));
    await waitFor(() =>
      expect(screen.getByLabelText('Level current automation value')).toHaveTextContent(
        'Automation: 0.2',
      ),
    );
    sidechainRenders.count = 0;

    act(() => {
      store.getState().transport.seekLocally(480);
    });

    await waitFor(() =>
      expect(screen.getByLabelText('Level current automation value')).toHaveTextContent(
        'Automation: 0.5',
      ),
    );
    expect(sidechainRenders.count).toBe(0);
    expect(api.calls.filter((call) => call === 'inspectDevice')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'listDeviceParameters')).toHaveLength(1);
  });

  it('keeps device metadata loaded when the same owner is rendered again', async () => {
    const api = runtimeApi();
    const { rerender } = renderDevices({ api });
    fireEvent.click(screen.getByRole('button', { name: effect.name }));
    await screen.findByRole('slider', { name: 'Level' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Reset Level' })).toBeEnabled());

    rerender();
    await act(async () => undefined);

    expect(api.calls.filter((call) => call === 'inspectDevice')).toHaveLength(1);
    expect(api.calls.filter((call) => call === 'listDeviceParameters')).toHaveLength(1);
  });

  it.each(['track', 'bus', 'master'] as const)(
    'commits parameters, presets and editor actions on %s',
    async (kind) => {
      const api = runtimeApi();
      const currentSession = defaultSession();
      currentSession.arrangement.tracks = [structuredClone(track)];
      currentSession.arrangement.automationLanes = [
        {
          id: 'automation:device-level',
          target: { kind: 'deviceParameter', deviceId: effect.id, parameterId: 'level' },
          interpolation: 'linear',
          points: [{ id: 'point:device-level', tick: 0, value: 0.75 }],
        },
      ];
      const owner: EffectOwner =
        kind === 'master' ? { kind } : { kind, id: kind === 'track' ? track.id : 'bus:1' };
      if (kind === 'bus')
        currentSession.arrangement.mixer.buses.push({
          id: 'bus:1',
          name: 'Group',
          kind: 'group',
          gainDb: 0,
          pan: 0,
          panLaw: 'equalPower',
          muted: false,
          solo: false,
          effects: [structuredClone(effect)],
        });
      if (kind === 'master')
        currentSession.arrangement.mixer.masterEffects = [structuredClone(effect)];
      const currentEffect = resolveDeviceTarget(currentSession.arrangement, owner)!.effects[0];

      const currentParameters = structuredClone(parameters);
      api.formatDeviceParameterValue = vi.fn(async () => '-3 dB');
      let releaseModeMetadata!: () => void;
      const modeMetadata = new Promise<void>((resolve) => {
        releaseModeMetadata = resolve;
      });
      let refreshingMode = false;
      const enabledDuringRefresh: boolean[] = [];
      const parameterReads = vi.fn(async () => {
        if (currentParameters[1].value === 1) await modeMetadata;
        return structuredClone(currentParameters);
      });
      api.listDeviceParameters = parameterReads;
      api.setDeviceParameter = vi.fn(async (_trackId, _deviceId, index, value) => {
        currentParameters[index] = {
          ...currentParameters[index],
          value,
          displayValue: String(value),
        };
        currentEffect.plugin.parameterValues[index] = value;
        if (index === 1) refreshingMode = true;
        return api.mutationResult(structuredClone(currentSession));
      });
      api.setPluginPreset = vi.fn(async () => {
        currentParameters.forEach((parameter) => {
          parameter.value = parameter.defaultValue;
        });
        currentEffect.plugin.parameterValues = currentParameters.map(
          (parameter) => parameter.value,
        );
        api.getPluginPreset = vi.fn().mockResolvedValue({ index: 1, name: 'Soft' });
        return api.mutationResult(structuredClone(currentSession));
      });
      api.openPluginEditor = vi.fn().mockResolvedValue(undefined);
      renderDevices({
        api,
        initialSession: structuredClone(currentSession),
        owner,
        wrapper: (content) => (
          <Profiler
            id="device-editor"
            onRender={() => {
              const reset = screen.queryByRole('button', { name: 'Reset Level' });
              if (refreshingMode && reset)
                enabledDuringRefresh.push(!reset.hasAttribute('disabled'));
            }}
          >
            {content}
          </Profiler>
        ),
      });
      if (kind === 'track') expect(screen.getByText('Audio Input →')).toBeInTheDocument();
      else expect(screen.queryByText('Audio Input →')).not.toBeInTheDocument();
      // The first device is selected automatically and exposes a direct editor button on its card.
      fireEvent.click(screen.getByRole('button', { name: 'Open Test Effect editor' }));
      expect(api.openPluginEditor).toHaveBeenCalledWith(owner, effect.id);
      const slider = await screen.findByLabelText('Level · AUTO');
      expect(screen.getByText('-6 dB')).toBeInTheDocument();
      await waitFor(() =>
        expect(screen.getByLabelText('Level current automation value')).toHaveTextContent(
          'Automation: -3 dB',
        ),
      );
      expect(api.formatDeviceParameterValue).toHaveBeenCalledWith(owner, effect.id, 0, 0.75);
      fireEvent.change(slider, { target: { value: '0.7' } });
      expect(api.setDeviceParameter).not.toHaveBeenCalled();
      fireEvent.pointerUp(slider);
      fireEvent.blur(slider);
      await waitFor(() => expect(api.setDeviceParameter).toHaveBeenCalledTimes(1));
      expect(api.setDeviceParameter).toHaveBeenLastCalledWith(owner, effect.id, 0, 0.7);
      expect(api.calls.filter((call) => call === 'inspectDevice')).toHaveLength(1);
      expect(api.calls.filter((call) => call === 'listPluginPresets')).toHaveLength(1);
      await waitFor(() => expect(screen.getByLabelText('Mode')).toBeEnabled());
      fireEvent.change(screen.getByLabelText('Mode'), { target: { value: '1' } });
      await waitFor(() =>
        expect(api.setDeviceParameter).toHaveBeenLastCalledWith(owner, effect.id, 1, 1),
      );
      expect(screen.getByRole('button', { name: 'Reset Level' })).toBeDisabled();
      expect(enabledDuringRefresh).not.toContain(true);
      refreshingMode = false;
      await act(async () => releaseModeMetadata());
      await waitFor(() =>
        expect(screen.getByRole('button', { name: 'Reset Level' })).toBeEnabled(),
      );
      expect(api.formatDeviceParameterValue).toHaveBeenCalledTimes(1);
      fireEvent.click(screen.getByRole('button', { name: 'Reset Level' }));
      await waitFor(() =>
        expect(api.setDeviceParameter).toHaveBeenLastCalledWith(owner, effect.id, 0, 0.25),
      );
      await waitFor(() => expect(screen.getByLabelText('Preset')).toBeEnabled());
      const reads = parameterReads.mock.calls.length;
      fireEvent.change(screen.getByLabelText('Preset'), { target: { value: '1' } });
      await waitFor(() => expect(api.setPluginPreset).toHaveBeenCalledWith(owner, effect.id, 1));
      await waitFor(() => expect(parameterReads.mock.calls.length).toBeGreaterThan(reads));
      await waitFor(() => expect(screen.getByLabelText('Preset')).toHaveValue('1'));
      await waitFor(() => expect(screen.getByLabelText('Level · AUTO')).toBeEnabled());
      api.setDeviceParameter = vi.fn().mockRejectedValue(new Error('Parameter rejected'));
      fireEvent.change(screen.getByLabelText('Level · AUTO'), { target: { value: '0.9' } });
      fireEvent.pointerUp(screen.getByLabelText('Level · AUTO'));
      expect(await screen.findByRole('alert')).toHaveTextContent('Parameter rejected');
      expect(screen.getByLabelText('Level · AUTO')).toHaveValue('0.25');
    },
  );

  it('ignores a parameter response after the device is deleted and keeps errors on the selected owner', async () => {
    // Arrange: a device whose parameter read stays pending.
    const api = runtimeApi();
    let resolve!: (value: DeviceParameterInfo[]) => void;
    api.listDeviceParameters = vi.fn(
      () =>
        new Promise<DeviceParameterInfo[]>((done) => {
          resolve = done;
        }),
    );
    const session = defaultSession();
    session.arrangement.tracks = [structuredClone(track)];
    const { store } = renderDevices({ api, initialSession: structuredClone(session) });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    await waitFor(() => expect(api.listDeviceParameters).toHaveBeenCalled());

    // Act: the device is removed while its parameters are loading.
    act(() => {
      const withoutEffect = structuredClone(session);
      withoutEffect.arrangement.tracks[0].effects = [];
      store.getState().project.acceptCanonical(api.mutationResult(withoutEffect).canonical);
    });
    await act(async () => resolve(parameters));

    // Assert: the late response is not shown.
    expect(screen.queryByLabelText('Level')).not.toBeInTheDocument();

    // Act: the device returns and its inspection fails.
    api.listDeviceParameters = vi.fn().mockRejectedValue(new Error('Inspection unavailable'));
    act(() => {
      store
        .getState()
        .project.acceptCanonical(api.mutationResult(structuredClone(session)).canonical);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));

    // Assert: the card stays and reports the failure.
    expect(await screen.findByRole('alert')).toHaveTextContent('Inspection unavailable');
    expect(screen.getByRole('button', { name: 'Test Effect' })).toBeInTheDocument();

    // Act: a device command fails on the selected channel.
    api.setDeviceBypassed = vi.fn().mockRejectedValue(new Error('Bypass rejected'));
    fireEvent.click(screen.getByRole('button', { name: 'Bypass' }));
    expect(await screen.findByText('Bypass rejected')).toBeInTheDocument();

    // Act: the selection moves to the master.
    inlineMessageRenders.count = 0;
    act(() => {
      store.getState().workspace.setSelection(ownerSelection({ kind: 'master' }));
    });

    // Assert: the previous owner's error is not rendered again.
    expect(inlineMessageRenders.count).toBe(0);
  });

  it('shows built-in and missing device controls without querying VST3 parameters', async () => {
    const api = runtimeApi(undefined, [
      {
        id: 'builtin:keys',
        presetId: 'keys',
        origin: 'builtIn',
        name: 'Built-in Keys',
        author: null,
        description: null,
        defaultCategory: 'Keys',
        category: 'Keys',
        defaultTags: [],
        userTags: [],
        tags: [],
        favorite: false,
        collectionIds: [],
        recommendedRange: { minMidi: 48, maxMidi: 84 },
        preview: null,
      },
    ]);
    const builtin: Track = {
      ...track,
      kind: 'instrument',
      instrument: {
        id: 'builtin:1',
        name: 'Built-in Keys',
        bypassed: false,
        midi: { transposeSemitones: 0, playableNotes: null, keyswitches: [] },
        source: {
          type: 'internal',
          definitionJson: '{}',
          resource: { type: 'builtInPreset', presetId: 'keys' },
        },
      },
      effects: [{ ...effect, plugin: { ...effect.plugin, disabledPlaceholder: true } }],
    };
    renderDevices({ api, initialTrack: builtin });
    expect(await screen.findByText(/Library recommendation:/)).toBeInTheDocument();
    expect(screen.getByLabelText('Playable notes')).toHaveValue('none');
    fireEvent.click(screen.getByRole('button', { name: 'Built-in Keys' }));
    expect(screen.getByRole('button', { name: 'Change' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Clear instrument' })).toBeInTheDocument();
    expect(screen.getByText('DISABLED PLACEHOLDER')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    expect(api.calls.filter((call) => call === 'inspectDevice')).toHaveLength(0);
  });

  it('shows a bypassed device placeholder without querying its plugin', () => {
    const api = runtimeApi();
    const bypassed: Track = {
      ...structuredClone(track),
      effects: [{ ...structuredClone(effect), bypassed: true }],
    };
    renderDevices({ api, initialTrack: bypassed });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    expect(screen.getByText('Bypassed · unbypass to edit this device.')).toBeInTheDocument();
    expect(api.calls.filter((call) => call === 'inspectDevice')).toHaveLength(0);
  });

  it('edits sidechain routes for a capable device', async () => {
    const api = runtimeApi({ available: true, mono: false, stereo: true, channels: 2 });
    const currentSession = defaultSession();
    currentSession.arrangement.tracks = [
      structuredClone(track),
      { ...structuredClone(track), id: 'track:2', name: 'Drums', effects: [] },
    ];
    currentSession.arrangement.mixer.sidechainSends = [
      {
        id: 'sidechain:1',
        sourceTrackId: 'track:2',
        targetDeviceId: effect.id,
        levelDb: -6,
        position: 'postFader',
        enabled: true,
      },
    ];
    const mutation = () => api.mutationResult(structuredClone(currentSession));
    api.previewSidechainSendLevel = vi.fn().mockResolvedValue(undefined);
    api.updateSidechainSend = vi.fn(async (_id, patch) => {
      if (patch.levelDb !== undefined && patch.levelDb !== null)
        currentSession.arrangement.mixer.sidechainSends[0].levelDb = patch.levelDb;
      return mutation();
    });
    api.removeSidechainSend = vi.fn(async () => {
      currentSession.arrangement.mixer.sidechainSends = [];
      return mutation();
    });
    const { store } = renderDevices({
      api,
      initialSession: structuredClone(currentSession),
    });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    expect(await screen.findByText('Sidechain input · Stereo · 2 channels')).toBeInTheDocument();
    expect(screen.getByLabelText('Add Sidechain source')).toBeDisabled();
    expect(screen.getByText('All Tracks already feed this device.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Automation' }));
    expect(store.getState().workspace.automation.trackTargets['track:2']).toEqual({
      kind: 'sidechainSendLevel',
      sidechainSendId: 'sidechain:1',
    });
    const slider = screen.getByLabelText('Drums sidechain level');
    fireEvent.pointerDown(slider, { pointerId: 1 });
    fireEvent.change(slider, { target: { value: '-12' } });
    fireEvent.pointerUp(slider, { pointerId: 1 });
    await waitFor(() =>
      expect(api.updateSidechainSend).toHaveBeenCalledWith('sidechain:1', { levelDb: -12 }),
    );
    expect(api.previewSidechainSendLevel).toHaveBeenCalledWith('sidechain:1', -12);
    fireEvent.click(screen.getByRole('button', { name: 'Remove Sidechain' }));
    await waitFor(() => expect(api.removeSidechainSend).toHaveBeenCalledWith('sidechain:1'));
    const sidechainSelect = screen.getByLabelText('Add Sidechain source');
    await waitFor(() => expect(sidechainSelect).toBeEnabled());
    expect(within(sidechainSelect).getByRole('option', { name: 'Drums' })).toBeInTheDocument();
    expect(
      within(sidechainSelect).queryByRole('option', { name: 'Audio' }),
    ).not.toBeInTheDocument();
  });

  it('disables adding a sidechain source when no mono or stereo layout is supported', async () => {
    const reason = 'The VST3 does not support a mono or stereo auxiliary input.';
    const api = runtimeApi({
      available: true,
      mono: false,
      stereo: false,
      channels: 0,
      reason,
    });
    const currentSession = defaultSession();
    currentSession.arrangement.tracks = [
      structuredClone(track),
      { ...structuredClone(track), id: 'track:2', name: 'Drums', effects: [] },
    ];

    renderDevices({ api, initialSession: structuredClone(currentSession) });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));

    expect(
      await screen.findByText('Sidechain input · Unavailable · 0 channels'),
    ).toBeInTheDocument();
    expect(screen.getByText(reason)).toBeInTheDocument();
    expect(screen.getByLabelText('Add Sidechain source')).toBeDisabled();
  });

  it('keeps a stored sidechain route visible and removable when the effect has no auxiliary bus', async () => {
    const reason = 'The VST3 does not expose an auxiliary input bus.';
    const api = runtimeApi({
      available: false,
      mono: false,
      stereo: false,
      channels: 0,
      reason,
    });
    const currentSession = defaultSession();
    currentSession.arrangement.tracks = [
      structuredClone(track),
      { ...structuredClone(track), id: 'track:2', name: 'Drums', effects: [] },
    ];
    currentSession.arrangement.mixer.sidechainSends = [
      {
        id: 'sidechain:stored',
        sourceTrackId: 'track:2',
        targetDeviceId: effect.id,
        levelDb: -6,
        position: 'postFader',
        enabled: true,
      },
    ];

    renderDevices({ api, initialSession: structuredClone(currentSession) });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));

    expect(
      await screen.findByText('Sidechain input · Unavailable · 0 channels'),
    ).toBeInTheDocument();
    expect(screen.getByText(reason)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove Sidechain' })).toBeInTheDocument();
    expect(screen.getByLabelText('Add Sidechain source')).toBeDisabled();
  });
});
