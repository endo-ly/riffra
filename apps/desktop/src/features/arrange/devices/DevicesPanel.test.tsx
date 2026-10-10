// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Profiler, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DeviceParameterInfo, Track } from '@/model/domain';
import { canonicalState, defaultSession } from '@/native/browser-defaults';
import { FakeNativeApi } from '@/native/native-api-fake';
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
      inspectTrackDevice: {
        id: effect.id,
        name: effect.name,
        source: 'vst3',
        bypassed: false,
        capabilities: { parameters: true, presets: true, editor: true, state: true },
        parameterCount: 2,
        statePersisted: false,
      },
      listTrackDeviceParameters: parameters,
      listTrackPluginPresets: [
        { index: 0, name: 'Init' },
        { index: 1, name: 'Soft' },
      ],
      getTrackPluginPreset: { index: 0, name: 'Init' },
    },
  });
}
function Harness({ api, initialTrack = track }: { api: FakeNativeApi; initialTrack?: Track }) {
  const initial = defaultSession();
  initial.arrangement.tracks = [structuredClone(initialTrack)];
  const [session, setSession] = useState(initial);
  return (
    <DevicesPanel
      projectId={session.sessionId}
      track={session.arrangement.tracks[0]}
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
  it('commits the final slider value once, uses choices/defaults, and refreshes after presets', async () => {
    const api = runtimeApi();
    const currentSession = defaultSession();
    currentSession.arrangement.tracks = [structuredClone(track)];
    const currentParameters = structuredClone(parameters);
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
    api.listTrackDeviceParameters = parameterReads;
    api.setTrackDeviceParameter = vi.fn(async (_trackId, _deviceId, index, value) => {
      currentParameters[index] = {
        ...currentParameters[index],
        value,
        displayValue: String(value),
      };
      currentSession.arrangement.tracks[0].effects[0].plugin.parameterValues[index] = value;
      if (index === 1) refreshingMode = true;
      return {
        canonical: canonicalState(structuredClone(currentSession)),
        createdEntityIds: {},
        projection: { state: 'notRequired' as const },
      };
    });
    api.setTrackPluginPreset = vi.fn(async () => {
      currentParameters.forEach((parameter) => {
        parameter.value = parameter.defaultValue;
      });
      currentSession.arrangement.tracks[0].effects[0].plugin.parameterValues =
        currentParameters.map((parameter) => parameter.value);
      api.getTrackPluginPreset = vi.fn().mockResolvedValue({ index: 1, name: 'Soft' });
      return {
        canonical: canonicalState(structuredClone(currentSession)),
        createdEntityIds: {},
        projection: { state: 'notRequired' as const },
      };
    });
    api.openTrackPluginEditor = vi.fn().mockResolvedValue(undefined);
    render(
      <Profiler
        id="device-editor"
        onRender={() => {
          const reset = screen.queryByRole('button', { name: 'Reset Level' });
          if (refreshingMode && reset) enabledDuringRefresh.push(!reset.hasAttribute('disabled'));
        }}
      >
        <Harness api={api} />
      </Profiler>,
    );
    expect(screen.getByText('Audio Input →')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test Effect' }));
    const slider = await screen.findByLabelText('Level');
    expect(screen.getByText('-6 dB')).toBeInTheDocument();
    fireEvent.change(slider, { target: { value: '0.7' } });
    expect(api.setTrackDeviceParameter).not.toHaveBeenCalled();
    fireEvent.pointerUp(slider);
    fireEvent.blur(slider);
    await waitFor(() => expect(api.setTrackDeviceParameter).toHaveBeenCalledTimes(1));
    expect(api.setTrackDeviceParameter).toHaveBeenLastCalledWith(track.id, effect.id, 0, 0.7);
    await waitFor(() => expect(screen.getByLabelText('Mode')).toBeEnabled());
    fireEvent.change(screen.getByLabelText('Mode'), { target: { value: '1' } });
    await waitFor(() =>
      expect(api.setTrackDeviceParameter).toHaveBeenLastCalledWith(track.id, effect.id, 1, 1),
    );
    expect(screen.getByRole('button', { name: 'Reset Level' })).toBeDisabled();
    expect(enabledDuringRefresh).not.toContain(true);
    refreshingMode = false;
    await act(async () => releaseModeMetadata());
    await waitFor(() => expect(screen.getByRole('button', { name: 'Reset Level' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Reset Level' }));
    await waitFor(() =>
      expect(api.setTrackDeviceParameter).toHaveBeenLastCalledWith(track.id, effect.id, 0, 0.25),
    );
    await waitFor(() => expect(screen.getByLabelText('Preset')).toBeEnabled());
    const reads = parameterReads.mock.calls.length;
    fireEvent.change(screen.getByLabelText('Preset'), { target: { value: '1' } });
    await waitFor(() =>
      expect(api.setTrackPluginPreset).toHaveBeenCalledWith(track.id, effect.id, 1),
    );
    await waitFor(() => expect(parameterReads.mock.calls.length).toBeGreaterThan(reads));
    await waitFor(() => expect(screen.getByLabelText('Preset')).toHaveValue('1'));
    fireEvent.click(screen.getByRole('button', { name: 'Open Plugin Editor' }));
    expect(api.openTrackPluginEditor).toHaveBeenCalledWith(track.id, effect.id);
    api.setTrackDeviceParameter = vi.fn().mockRejectedValue(new Error('Parameter rejected'));
    fireEvent.change(screen.getByLabelText('Level'), { target: { value: '0.9' } });
    fireEvent.pointerUp(screen.getByLabelText('Level'));
    expect(await screen.findByRole('alert')).toHaveTextContent('Parameter rejected');
    expect(screen.getByLabelText('Level')).toHaveValue('0.25');
  });

  it('ignores a parameter response after the device is deleted and leaves inspection errors on its card', async () => {
    const api = runtimeApi();
    let resolve!: (value: DeviceParameterInfo[]) => void;
    api.listTrackDeviceParameters = vi.fn(
      () =>
        new Promise<DeviceParameterInfo[]>((done) => {
          resolve = done;
        }),
    );
    const props = {
      projectId: 'project:1',
      track,
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
    await waitFor(() => expect(api.listTrackDeviceParameters).toHaveBeenCalled());
    rerender(<DevicesPanel {...props} track={{ ...track, effects: [] }} />);
    await act(async () => resolve(parameters));
    expect(screen.queryByLabelText('Level')).not.toBeInTheDocument();
    api.listTrackDeviceParameters = vi.fn().mockRejectedValue(new Error('Inspection unavailable'));
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
    expect(api.calls.filter((call) => call === 'inspectTrackDevice')).toHaveLength(0);
  });
});
