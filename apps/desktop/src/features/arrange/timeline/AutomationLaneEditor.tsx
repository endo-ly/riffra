import { useEffect, useState, type MutableRefObject } from 'react';
import type {
  AutomationInterpolation,
  AutomationPoint,
  AutomationTarget,
  CreativeSession,
  DeviceParameterInfo,
  EffectOwner,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import {
  automationDevices,
  automationOptions,
  automationOwner,
  automationTargetKey,
} from '../model/automation-targets';
import { AutomationLaneView } from './AutomationLaneView';

interface Props {
  session: CreativeSession;
  target: AutomationTarget;
  api: ArrangeWorkspaceApi;
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
  const owner = automationOwner(props.session.arrangement, props.target);
  const ownerKey = JSON.stringify(owner);
  const devices = owner ? automationDevices(props.session, owner) : [];
  const deviceKey = JSON.stringify(devices.map((device) => [device.id, device.plugin]));
  const [parameters, setParameters] = useState<Record<string, DeviceParameterInfo[]>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    setParameters({});
    setError(null);
    const queryOwner = JSON.parse(ownerKey) as EffectOwner | null;
    if (!queryOwner) return;
    const available = (
      JSON.parse(deviceKey) as [string, { disabledPlaceholder: boolean }][]
    ).filter(([, plugin]) => !plugin.disabledPlaceholder);
    void Promise.allSettled(
      available.map(
        async ([id]) => [id, await props.api.listDeviceParameters(queryOwner, id)] as const,
      ),
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
  }, [ownerKey, deviceKey, props.session.sessionId, props.api]);
  if (!owner) return null;
  const options = automationOptions(props.session, owner, parameters);
  const key = automationTargetKey(props.target);
  const deviceTarget = props.target.kind === 'deviceParameter' ? props.target : null;
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
  const formattedParameter = deviceTarget
    ? (parameters[deviceTarget.deviceId] ?? []).find(
        (parameter) => parameter.parameterId === deviceTarget.parameterId,
      )
    : undefined;
  const formatValue = (value: number) => {
    if (!owner || !formattedParameter || !deviceTarget) return Promise.resolve(value.toFixed(3));
    return props.api.formatDeviceParameterValue(
      owner,
      deviceTarget.deviceId,
      formattedParameter.index,
      value,
    );
  };
  const lane = props.session.arrangement.automationLanes.find(
    (lane) => automationTargetKey(lane.target) === key,
  );
  return (
    <AutomationLaneView
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
