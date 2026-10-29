import { errorText } from '@/shared/errors';
import { useEffect, useRef, useState } from 'react';
import type {
  ArrangementMutationResult,
  DeviceInspection,
  EffectOwner,
  DeviceParameterInfo,
  PluginPresetInfo,
} from '@/model/generated';
import { HostConnectionChangedError } from '@/native/invoke';
import { useSessionContext } from '@/shared/session/session-context';

interface DeviceDetails {
  inspection: DeviceInspection;
  parameters: DeviceParameterInfo[];
  presets: PluginPresetInfo[];
  currentPreset: PluginPresetInfo | null;
}

export function useDeviceDetails(
  owner: EffectOwner,
  deviceId: string,
  parameterValues: number[],
  stateData: string | undefined,
) {
  const { api, commit: sessionCommit } = useSessionContext();
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
        setError(errorText(failure));
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
      const guarded = operation().then((result) => {
        if (sequence.current !== request) throw new HostConnectionChangedError();
        return result;
      });
      const result = await sessionCommit(guarded, {
        onError: (message) => {
          if (sequence.current === request) setError(message);
        },
      });
      if (!result || sequence.current !== request) return false;
      setLoading(true);
      setRefresh((value) => value + 1);
      return true;
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
        setError(errorText(failure));
    }
  };
  return { details, loading, error, saving, mutate, openEditor };
}
