import { useCallback, useEffect, useMemo, useState, type MutableRefObject } from 'react';
import type {
  AutomationInterpolation,
  AutomationPoint,
  AutomationTarget,
  CreativeSession,
  DeviceParameterInfo,
  EffectOwner,
} from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import {
  automationDevices,
  automationOptions,
  automationOwner,
  automationTargetKey,
} from '@/model/automation';
import { AutomationLaneView } from './AutomationLaneView';

interface Props {
  session: CreativeSession;
  target: AutomationTarget;
  warnings: readonly string[];
  timelineWidth: number;
  pixelsPerTick: number;
  playheadTick: number;
  playheadTickRef: MutableRefObject<number>;
  previewScope: string;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  onTarget: (target: AutomationTarget) => void;
  onClose: () => void;
  onCommit: (
    target: AutomationTarget,
    interpolation: AutomationInterpolation,
    points: AutomationPoint[],
  ) => void | Promise<unknown>;
  onPreview: (
    target: AutomationTarget,
    value: number | null,
    token: number,
  ) => void | Promise<void>;
}

export function AutomationLaneEditor(props: Props) {
  const { api } = useSessionContext();
  const ownerCandidate = automationOwner(props.session.arrangement, props.target);
  const ownerKind = ownerCandidate?.kind ?? null;
  const ownerId = ownerCandidate && 'id' in ownerCandidate ? ownerCandidate.id : null;
  const owner = useMemo<EffectOwner | null>(() => {
    if (ownerKind === 'master') return { kind: 'master' };
    return ownerKind && ownerId !== null ? { kind: ownerKind, id: ownerId } : null;
  }, [ownerKind, ownerId]);
  const devices = owner ? automationDevices(props.session, owner) : [];
  const deviceKey = JSON.stringify(devices.map((device) => [device.id, device.plugin]));
  const [parameters, setParameters] = useState<Record<string, DeviceParameterInfo[]>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    setParameters({});
    setError(null);
    if (!owner) return;
    const available = (
      JSON.parse(deviceKey) as [string, { disabledPlaceholder: boolean }][]
    ).filter(([, plugin]) => !plugin.disabledPlaceholder);
    void Promise.allSettled(
      available.map(async ([id]) => [id, await api.listDeviceParameters(owner, id)] as const),
    ).then((results) => {
      if (disposed) return;
      const entries: [string, DeviceParameterInfo[]][] = [];
      for (const result of results) {
        if (result.status === 'fulfilled') entries.push([result.value[0], result.value[1]]);
        else
          setError(result.reason instanceof Error ? result.reason.message : String(result.reason));
      }
      setParameters(Object.fromEntries(entries));
    });
    return () => {
      disposed = true;
    };
  }, [owner, deviceKey, props.session.sessionId, api]);
  const deviceId = props.target.kind === 'deviceParameter' ? props.target.deviceId : undefined;
  const parameterId =
    props.target.kind === 'deviceParameter' ? props.target.parameterId : undefined;
  const parameterIndex =
    deviceId && parameterId
      ? (parameters[deviceId] ?? []).find((parameter) => parameter.parameterId === parameterId)
          ?.index
      : undefined;
  const formatValue = useCallback(
    (value: number) => {
      if (!owner || !deviceId || parameterIndex === undefined)
        return Promise.resolve(value.toFixed(3));
      return api.formatDeviceParameterValue(owner, deviceId, parameterIndex, value);
    },
    [api, owner, deviceId, parameterIndex],
  );
  if (!owner) return null;
  const options = automationOptions(props.session, owner, parameters);
  const key = automationTargetKey(props.target);
  let selected = options.find((option) => automationTargetKey(option.target) === key);
  if (!selected && props.target.kind === 'deviceParameter') {
    selected = {
      target: props.target,
      name: `${props.target.deviceId} · ${props.target.parameterId}`,
      staticValue: undefined,
      min: 0,
      max: 1,
      unit: 'normalized',
    };
    options.push(selected);
  }
  if (!selected) return null;
  const lane = props.session.arrangement.automationLanes.find(
    (lane) => automationTargetKey(lane.target) === key,
  );
  return (
    <AutomationLaneView
      timebase={props.session.arrangement.timebase}
      key={key}
      option={selected}
      options={options}
      lane={lane}
      error={
        props.target.kind === 'deviceParameter'
          ? (props.warnings.find(
              (warning) =>
                props.target.kind === 'deviceParameter' &&
                warning.startsWith(`${props.target.deviceId}/${props.target.parameterId}:`),
            ) ?? error)
          : error
      }
      timelineWidth={props.timelineWidth}
      pixelsPerTick={props.pixelsPerTick}
      playheadTick={props.playheadTick}
      playheadTickRef={props.playheadTickRef}
      previewScope={props.previewScope}
      snapTick={props.snapTick}
      onTarget={props.onTarget}
      onClose={props.onClose}
      onPreview={(value, token) => props.onPreview(props.target, value, token)}
      formatValue={props.target.kind === 'deviceParameter' ? formatValue : undefined}
      onCommit={(points, interpolation) => props.onCommit(props.target, interpolation, points)}
    />
  );
}
