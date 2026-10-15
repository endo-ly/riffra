// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Profiler, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CreativeSession, EffectOwner, DeviceParameterInfo, Track } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
import { resolveDeviceTarget } from './device-target';
import { DevicesPanel } from './DevicesPanel';

afterEach(cleanup);
const effect = {
  id: 'effect:1',
  name: 'Test Effect',
  bypassed: false,
  plugin: { path: 'test.vst3', parameterValues: [0.5, 0], disabledPlaceholder: false },
};
const track: Track = {
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
function runtimeApi() {
  return new FakeNativeApi({
    responses: {
      inspectDevice: {
        id: effect.id,
        name: effect.name,
        source: 'vst3',
        bypassed: false,
        capabilities: { parameters: true, presets: true, editor: true, state: true },
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
  owner = { kind: 'track', id: track.id },
}: {
  api: FakeNativeApi;
  initialTrack?: Track;
  initialSession?: CreativeSession;
  owner?: EffectOwner;
}) {
  const initial = initialSession ?? defaultSession();
  if (!initialSession) initial.arrangement.tracks = [structuredClone(initialTrack)];
  const [session, setSession] = useState(initial);
  return (
    <DevicesPanel
      projectId={session.sessionId}
      automationLanes={session.arrangement.automationLanes}
      playing={false}
      currentTick={0}
      onOpenAutomation={() => undefined}
      target={resolveDeviceTarget(session.arrangement, owner)}
      api={api}
      applyCanonicalState={(canonical) => {
        setSession(canonical.session);
        return true;
      }}
      plugins={[]}
      instruments={[]}
      missingDeviceIds={[]}
      onDisableMissingPlugin={async () => undefined}
      onReplaceMissingPlugin={async () => undefined}
      onRescanMissingPlugins={async () => undefined}
    />
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
        return {
          canonical: canonicalState(structuredClone(currentSession)),
          createdEntityIds: {},
          projection: { state: 'notRequired' as const },
        };
      });
      api.setPluginPreset = vi.fn(async () => {
        currentParameters.forEach((parameter) => {
          parameter.value = parameter.defaultValue;
        });
        currentEffect.plugin.parameterValues = currentParameters.map(
          (parameter) => parameter.value,
        );
        api.getPluginPreset = vi.fn().mockResolvedValue({ index: 1, name: 'Soft' });
        return {
          canonical: canonicalState(structuredClone(currentSession)),
          createdEntityIds: {},
          projection: { state: 'notRequired' as const },
        };
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
      projectId: 'project:1',
      automationLanes: [],
      playing: false,
      currentTick: 0,
      onOpenAutomation: () => undefined,
      target: {
        owner: { kind: 'track' as const, id: track.id },
        name: track.name,
        track,
        effects: track.effects,
      },
      api,
      applyCanonicalState: () => true,
      plugins: [],
      instruments: [],
      missingDeviceIds: [],
      onDisableMissingPlugin: async () => undefined,
      onReplaceMissingPlugin: async () => undefined,
      onRescanMissingPlugins: async () => undefined,
    };
    const { rerender } = render(<DevicesPanel {...props} />);
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    await waitFor(() => expect(api.listDeviceParameters).toHaveBeenCalled());
    rerender(
      <DevicesPanel
        {...props}
        target={{ ...props.target, track: { ...track, effects: [] }, effects: [] }}
      />,
    );
    await act(async () => resolve(parameters));
    expect(screen.queryByLabelText('Level')).not.toBeInTheDocument();
    api.listDeviceParameters = vi.fn().mockRejectedValue(new Error('Inspection unavailable'));
    rerender(<DevicesPanel {...props} />);
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
        source: {
          type: 'internal',
          definitionJson: '{}',
          resource: { type: 'builtInPreset', presetId: 'keys' },
        },
      },
      effects: [{ ...effect, plugin: { ...effect.plugin, disabledPlaceholder: true } }],
    };
    render(<Harness api={api} initialTrack={builtin} />);
    fireEvent.click(screen.getByRole('button', { name: 'Built-in Keys' }));
    expect(screen.getByRole('button', { name: 'Change' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Clear instrument' })).toBeInTheDocument();
    expect(screen.getByText('DISABLED PLACEHOLDER')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    expect(api.calls.filter((call) => call === 'inspectDevice')).toHaveLength(0);
  });
});
