import { useEffect, useRef, useState } from 'react';
import type {
  ArrangementMutationResult,
  CanonicalState,
  DeviceInspection,
  EffectOwner,
  DeviceParameterInfo,
  PluginPresetInfo,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import { HostConnectionChangedError } from '@/native/invoke';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface DeviceDetails {
  inspection: DeviceInspection;
  parameters: DeviceParameterInfo[];
  presets: PluginPresetInfo[];
  currentPreset: PluginPresetInfo | null;
}

export function useDeviceDetails(
  api: ArrangeWorkspaceApi,
  owner: EffectOwner,
  deviceId: string,
  parameterValues: number[],
  stateData: string | undefined,
  applyCanonicalState: (state: CanonicalState) => boolean,
) {
  const sequence = useRef(0);
  const parameterRevision = parameterValues.join(',');
  const [details, setDetails] = useState<DeviceDetails | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const request = ++sequence.current;
    setLoading(true);
    void api
      .inspectDevice(owner, deviceId)
      .then(async (inspection) => {
        if (sequence.current !== request) return;
        const [parameters, presets, currentPreset] = await Promise.all([
          inspection.capabilities.parameters ? api.listDeviceParameters(owner, deviceId) : [],
          inspection.capabilities.presets ? api.listPluginPresets(owner, deviceId) : [],
          inspection.capabilities.presets ? api.getPluginPreset(owner, deviceId) : null,
        ]);
        if (sequence.current === request)
          setDetails({ inspection, parameters, presets, currentPreset });
      })
      .catch((failure: unknown) => {
        if (sequence.current !== request || failure instanceof HostConnectionChangedError) return;
        setError(failure instanceof Error ? failure.message : String(failure));
        setDetails(null);
      })
      .finally(() => {
        if (sequence.current === request) setLoading(false);
      });
    return () => {
      sequence.current = request + 1;
    };
  }, [api, owner, deviceId, parameterRevision, stateData, refresh]);

  const mutate = async (operation: () => Promise<ArrangementMutationResult>) => {
    const request = sequence.current;
    setSaving(true);
    setError(null);
    try {
      const result = await operation();
      if (sequence.current !== request) return false;
      setLoading(true);
      applyArrangementMutation(result, applyCanonicalState, setError);
      setRefresh((value) => value + 1);
      return true;
    } catch (failure) {
      if (sequence.current === request && !(failure instanceof HostConnectionChangedError))
        setError(failure instanceof Error ? failure.message : String(failure));
      return false;
    } finally {
      // The component is keyed by Project / Track / Device. A metadata refresh
      // may supersede the mutation's read sequence without changing its target.
      setSaving(false);
    }
  };
  const openEditor = async () => {
    const request = sequence.current;
    try {
      await api.openPluginEditor(owner, deviceId);
    } catch (failure) {
      if (sequence.current === request && !(failure instanceof HostConnectionChangedError))
        setError(failure instanceof Error ? failure.message : String(failure));
    }
  };
  return { details, loading, error, saving, mutate, openEditor };
}
