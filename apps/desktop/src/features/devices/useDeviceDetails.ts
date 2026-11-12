import { errorText } from '@/shared/errors';
import { useEffect, useMemo, useRef, useState } from 'react';
import type {
  ArrangementMutationResult,
  DeviceInspection,
  EffectOwner,
  DeviceParameterInfo,
  PluginPresetInfo,
} from '@/model/generated';
import { HostConnectionChangedError } from '@/native/invoke';
import { useSession, useSessionContext } from '@/shared/session/session-context';

interface DeviceDetails {
  inspection: DeviceInspection;
  parameters: DeviceParameterInfo[];
  presets: PluginPresetInfo[];
  currentPreset: PluginPresetInfo | null;
}

export function useDeviceDetails(
  requestedOwner: EffectOwner,
  deviceId: string,
  parameterValues: number[],
  stateData: string | undefined,
) {
  const { api, commit: sessionCommit } = useSessionContext();
  const hostGeneration = useSession((state) => state.host.connection.generation);
  const ownerKind = requestedOwner.kind;
  const ownerId = 'id' in requestedOwner ? requestedOwner.id : null;
  const owner = useMemo<EffectOwner>(
    () => (ownerKind === 'master' ? { kind: 'master' } : { kind: ownerKind, id: ownerId! }),
    [ownerKind, ownerId],
  );
  const sequence = useRef(0);
  const targetGeneration = useRef(0);
  const parameterRevision = parameterValues.join(',');
  const [details, setDetails] = useState<DeviceDetails | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    targetGeneration.current += 1;
    setDetails(null);
    setError(null);
    setSaving(false);
    return () => {
      targetGeneration.current += 1;
    };
  }, [api, owner, deviceId, hostGeneration]);

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
  }, [api, owner, deviceId, hostGeneration, parameterRevision, stateData, refresh]);

  const mutate = async (operation: () => Promise<ArrangementMutationResult>) => {
    const target = targetGeneration.current;
    setSaving(true);
    setError(null);
    try {
      const result = await sessionCommit(operation(), {
        onError: (message) => {
          if (targetGeneration.current === target) setError(message);
        },
      });
      if (!result || targetGeneration.current !== target) return false;
      setLoading(true);
      setRefresh((value) => value + 1);
      return true;
    } finally {
      if (targetGeneration.current === target) setSaving(false);
    }
  };
  const openEditor = async () => {
    const target = targetGeneration.current;
    try {
      await api.openPluginEditor(owner, deviceId);
    } catch (failure) {
      if (targetGeneration.current === target && !(failure instanceof HostConnectionChangedError))
        setError(errorText(failure));
    }
  };
  return { details, loading, error, saving, mutate, openEditor };
}
