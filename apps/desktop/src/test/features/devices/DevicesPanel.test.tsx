import { createTestSessionStore, TestSessionProvider } from '@/test/support/session-provider';
// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Profiler, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AutomationTarget,
  CreativeSession,
  DeviceSidechainCapability,
  EffectOwner,
  DeviceParameterInfo,
  InstrumentLibraryItem,
  Track,
} from '@/model/generated';
import { defaultSession } from '@/native/browser-defaults';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { resolveDeviceTarget } from '@/features/devices/device-target';
import { DevicesPanelView } from '@/features/devices/DevicesPanel';

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
) {
  return createFakeNativeApi({
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
function Harness({
  api,
  initialTrack = track,
  initialSession,
  instruments = [],
  owner = { kind: 'track', id: track.id },
  onOpenAutomation = () => undefined,
}: {
  api: FakeNativeApi;
  initialTrack?: Track;
  initialSession?: CreativeSession;
  instruments?: InstrumentLibraryItem[];
  owner?: EffectOwner;
  onOpenAutomation?: (target: AutomationTarget) => void;
}) {
  const initial = initialSession ?? defaultSession();
  if (!initialSession) initial.arrangement.tracks = [structuredClone(initialTrack)];
  const [session, setSession] = useState(initial);
  const [store] = useState(() =>
    createTestSessionStore(api, {
      session: initial,
      applyCanonical: (canonical) => setSession(canonical.session),
    }),
  );
  return (
    <TestSessionProvider api={api} store={store}>
      <DevicesPanelView
        sessionId={session.sessionId}
        automationLanes={session.arrangement.automationLanes}
        arrangement={session.arrangement}
        playing={false}
        currentTick={0}
        onOpenAutomation={onOpenAutomation}
        target={resolveDeviceTarget(session.arrangement, owner)}
        plugins={[]}
        instruments={instruments}
        missingDeviceIds={[]}
        onDisableMissingPlugin={async () => undefined}
        onReplaceMissingPlugin={async () => undefined}
        onRescanMissingPlugins={async () => undefined}
      />
    </TestSessionProvider>
  );
}

describe('DevicesPanel', () => {
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
      render(
        <Profiler
          id="device-editor"
          onRender={() => {
            const reset = screen.queryByRole('button', { name: 'Reset Level' });
            if (refreshingMode && reset) enabledDuringRefresh.push(!reset.hasAttribute('disabled'));
          }}
        >
          <Harness api={api} initialSession={structuredClone(currentSession)} owner={owner} />
        </Profiler>,
      );
      if (kind === 'track') expect(screen.getByText('Audio Input →')).toBeInTheDocument();
      else expect(screen.queryByText('Audio Input →')).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
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
      fireEvent.click(screen.getByRole('button', { name: 'Open Plugin Editor' }));
      expect(api.openPluginEditor).toHaveBeenCalledWith(owner, effect.id);
      api.setDeviceParameter = vi.fn().mockRejectedValue(new Error('Parameter rejected'));
      fireEvent.change(screen.getByLabelText('Level · AUTO'), { target: { value: '0.9' } });
      fireEvent.pointerUp(screen.getByLabelText('Level · AUTO'));
      expect(await screen.findByRole('alert')).toHaveTextContent('Parameter rejected');
      expect(screen.getByLabelText('Level · AUTO')).toHaveValue('0.25');
    },
  );

  it('ignores a parameter response after the device is deleted and leaves inspection errors on its card', async () => {
    const api = runtimeApi();
    let resolve!: (value: DeviceParameterInfo[]) => void;
    api.listDeviceParameters = vi.fn(
      () =>
        new Promise<DeviceParameterInfo[]>((done) => {
          resolve = done;
        }),
    );
    const props = {
      sessionId: 'project:1',
      automationLanes: [],
      arrangement: defaultSession().arrangement,
      playing: false,
      currentTick: 0,
      onOpenAutomation: () => undefined,
      target: {
        owner: { kind: 'track' as const, id: track.id },
        name: track.name,
        track,
        effects: track.effects,
      },
      plugins: [],
      instruments: [],
      missingDeviceIds: [],
      onDisableMissingPlugin: async () => undefined,
      onReplaceMissingPlugin: async () => undefined,
      onRescanMissingPlugins: async () => undefined,
    };
    const { rerender } = render(<DevicesPanelView {...props} />, {
      wrapper: ({ children }) => <TestSessionProvider api={api}>{children}</TestSessionProvider>,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    await waitFor(() => expect(api.listDeviceParameters).toHaveBeenCalled());
    rerender(
      <DevicesPanelView
        {...props}
        target={{ ...props.target, track: { ...track, effects: [] }, effects: [] }}
      />,
    );
    await act(async () => resolve(parameters));
    expect(screen.queryByLabelText('Level')).not.toBeInTheDocument();
    api.listDeviceParameters = vi.fn().mockRejectedValue(new Error('Inspection unavailable'));
    rerender(<DevicesPanelView {...props} />);
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Inspection unavailable');
    expect(screen.getByRole('button', { name: 'Test Effect' })).toBeInTheDocument();
  });

  it('shows built-in and missing device controls without querying VST3 parameters', () => {
    const api = runtimeApi();
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
    render(
      <Harness
        api={api}
        initialTrack={builtin}
        instruments={[
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
        ]}
      />,
    );
    expect(screen.getByText(/Library recommendation:/)).toBeInTheDocument();
    expect(screen.getByLabelText('Playable notes')).toHaveValue('none');
    fireEvent.click(screen.getByRole('button', { name: 'Built-in Keys' }));
    expect(screen.getByRole('button', { name: 'Change' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Clear instrument' })).toBeInTheDocument();
    expect(screen.getByText('DISABLED PLACEHOLDER')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
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
    const opened: AutomationTarget[] = [];
    render(
      <Harness
        api={api}
        initialSession={structuredClone(currentSession)}
        onOpenAutomation={(target) => opened.push(target)}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    expect(await screen.findByText('Sidechain input · Stereo · 2 channels')).toBeInTheDocument();
    expect(screen.getByLabelText('Add Sidechain source')).toBeDisabled();
    expect(screen.getByText('All Tracks already feed this device.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Automation' }));
    expect(opened).toEqual([{ kind: 'sidechainSendLevel', sidechainSendId: 'sidechain:1' }]);
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
    await waitFor(() => expect(screen.getByLabelText('Add Sidechain source')).toBeEnabled());
    expect(screen.getByRole('option', { name: 'Drums' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'Audio' })).not.toBeInTheDocument();
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

    render(<Harness api={api} initialSession={structuredClone(currentSession)} />);
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

    render(<Harness api={api} initialSession={structuredClone(currentSession)} />);
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));

    expect(
      await screen.findByText('Sidechain input · Unavailable · 0 channels'),
    ).toBeInTheDocument();
    expect(screen.getByText(reason)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Remove Sidechain' })).toBeInTheDocument();
    expect(screen.getByLabelText('Add Sidechain source')).toBeDisabled();
  });
});
