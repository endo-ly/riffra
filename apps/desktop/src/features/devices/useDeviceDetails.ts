import { useSessionCommand } from '@/shared/session/useSessionCommand';
import { errorText } from '@/shared/errors';
import { useSessionRead } from '@/shared/session/useSessionRead';
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
  pluginPath: string,
  parameterValues: number[],
  stateData: string | undefined,
) {
  const { api } = useSessionContext();
  const sessionCommit = useSessionCommand(JSON.stringify([requestedOwner, deviceId, pluginPath]));
  const scope = useSession((state) => state.scope);
  const ownerKind = requestedOwner.kind;
  const ownerId = 'id' in requestedOwner ? requestedOwner.id : null;
  const owner = useMemo<EffectOwner>(
    () => (ownerKind === 'master' ? { kind: 'master' } : { kind: ownerKind, id: ownerId! }),
    [ownerKind, ownerId],
  );
  const targetGeneration = useRef(0);
  const parameterRevision = parameterValues.join(',');
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    targetGeneration.current += 1;
    setError(null);
    setSaving(false);
    return () => {
      targetGeneration.current += 1;
    };
  }, [api, owner, deviceId, pluginPath, scope]);

  const key = JSON.stringify([owner, deviceId, pluginPath]);
  const metadataRequest = useMemo(
    () => ({
      key: `device-metadata:${key}`,
      inputs: [stateData, refresh],
      load: async () => {
        const inspection = await api.inspectDevice(owner, deviceId);
        const presets = inspection.capabilities.presets
          ? await api.listPluginPresets(owner, deviceId)
          : [];
        return { inspection, presets };
      },
    }),
    [api, owner, deviceId, key, stateData, refresh],
  );
  const metadata = useSessionRead(metadataRequest, { keepPrevious: true });
  const valuesRequest = useMemo(
    () =>
      metadata.data
        ? {
            key: `device-values:${key}`,
            inputs: [metadata.data, parameterRevision, stateData, refresh],
            load: async () => {
              const capabilities = metadata.data!.inspection.capabilities;
              const [parameters, currentPreset] = await Promise.all([
                capabilities.parameters ? api.listDeviceParameters(owner, deviceId) : [],
                capabilities.presets ? api.getPluginPreset(owner, deviceId) : null,
              ]);
              return { parameters, currentPreset };
            },
          }
        : null,
    [api, owner, deviceId, key, metadata.data, parameterRevision, stateData, refresh],
  );
  const values = useSessionRead(valuesRequest, { keepPrevious: true, enabled: !metadata.loading });
  const details: DeviceDetails | null =
    metadata.data && values.data ? { ...metadata.data, ...values.data } : null;
  const loading = metadata.loading || values.loading;

  const mutate = async (
    operation: () => Promise<ArrangementMutationResult>,
    refreshMetadata = false,
  ) => {
    const target = targetGeneration.current;
    setSaving(true);
    setError(null);
    try {
      const result = await sessionCommit(operation(), {
        onError: setError,
      });
      if (!result) return false;
      if (refreshMetadata) setRefresh((value) => value + 1);
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
  return {
    details,
    loading,
    error: error ?? metadata.error ?? values.error,
    saving,
    mutate,
    openEditor,
  };
}
