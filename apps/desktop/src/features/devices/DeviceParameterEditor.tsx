import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import type {
  Arrangement,
  ArrangementMutationResult,
  AutomationLane,
  DeviceParameterInfo,
  EffectOwner,
} from '@/model/generated';
import { Icon } from '@/shared/ui/primitives';
import controls from '@/shared/ui/controls.module.css';
import { useSessionContext } from '@/shared/session/session-context';
import { useDeviceDetails } from './useDeviceDetails';
import { DeviceSidechainEditor } from './DeviceSidechainEditor';
import { automationValueAt } from '@/model/automation';
import { useAutomationValueDisplay } from '@/shared/useAutomationValueDisplay';
import styles from './Devices.module.css';

interface DeviceParameterEditorProps {
  owner: EffectOwner;
  deviceId: string;
  stateData?: string;
  parameterValues: number[];
  automationLanes: AutomationLane[];
  automationReadActive: boolean;
  currentTick: number;
  onOpenAutomation: (parameter: DeviceParameterInfo) => void;
  arrangement: Arrangement;
  isInstrument: boolean;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onOpenSidechainAutomation: (sidechainSendId: string) => void;
}

function ParameterControl({
  parameter,
  owner,
  deviceId,
  canonicalValue,
  disabled,
  onCommit,
  automated,
  onOpenAutomation,
  automationValue,
}: {
  parameter: DeviceParameterInfo;
  owner: EffectOwner;
  deviceId: string;
  canonicalValue: number;
  disabled: boolean;
  onCommit: (value: number) => Promise<boolean>;
  automated: boolean;
  onOpenAutomation: () => void;
  automationValue?: number;
}) {
  const { api } = useSessionContext();
  const [draft, setDraft] = useState(parameter.value);
  const ownerKind = owner.kind;
  const ownerId = 'id' in owner ? owner.id : null;
  const formatterOwner = useMemo<EffectOwner>(() => {
    if (ownerKind === 'master') return { kind: 'master' };
    return { kind: ownerKind, id: ownerId! };
  }, [ownerKind, ownerId]);
  const formatAutomationValue = useCallback(
    (value: number) =>
      api.formatDeviceParameterValue(formatterOwner, deviceId, parameter.index, value),
    [api, formatterOwner, deviceId, parameter.index],
  );
  const automationDisplay = useAutomationValueDisplay(automationValue, formatAutomationValue);
  const submitted = useRef(parameter.value);
  useEffect(() => {
    setDraft(parameter.value);
    submitted.current = parameter.value;
  }, [parameter]);
  const commit = async (value: number) => {
    if (disabled || value === submitted.current) return;
    submitted.current = value;
    const success = await onCommit(value);
    if (!success) {
      setDraft(canonicalValue);
      submitted.current = canonicalValue;
    }
  };
  const name = parameter.name || `Parameter ${parameter.index}`;
  const choiceIndex = parameter.choices.reduce(
    (closest, choice, index, choices) =>
      Math.abs(choice.value - draft) < Math.abs(choices[closest].value - draft) ? index : closest,
    0,
  );
  return (
    <div className={styles.parameter} data-automated={automated}>
      <label htmlFor={`parameter-${parameter.index}`} title={name}>
        {name} {automated ? '· AUTO' : ''}
      </label>
      {parameter.choices.length >= 2 ? (
        <select
          className={controls.control}
          id={`parameter-${parameter.index}`}
          disabled={disabled}
          value={choiceIndex}
          onChange={(event) => {
            const value = parameter.choices[Number(event.currentTarget.value)].value;
            setDraft(value);
            void commit(value);
          }}
        >
          {parameter.choices.map((choice, index) => (
            <option key={index} value={index}>
              {choice.displayValue}
            </option>
          ))}
        </select>
      ) : (
        <input
          className={controls.fillSlider}
          style={{ '--fill-start': '0%', '--fill-end': `${draft * 100}%` } as CSSProperties}
          id={`parameter-${parameter.index}`}
          type="range"
          min={0}
          max={1}
          step={
            parameter.discrete && parameter.stepCount >= 2 ? 1 / (parameter.stepCount - 1) : 0.001
          }
          value={draft}
          disabled={disabled}
          onChange={(event) => setDraft(Number(event.currentTarget.value))}
          onPointerUp={(event) => {
            void commit(Number(event.currentTarget.value));
          }}
          onPointerCancel={() => setDraft(canonicalValue)}
          onKeyUp={(event) => {
            if (
              [
                'ArrowLeft',
                'ArrowRight',
                'ArrowUp',
                'ArrowDown',
                'Home',
                'End',
                'PageUp',
                'PageDown',
              ].includes(event.key)
            )
              void commit(Number(event.currentTarget.value));
          }}
          onBlur={(event) => {
            void commit(Number(event.currentTarget.value));
          }}
        />
      )}
      <span className={styles.parameterValue}>
        <output title={`${canonicalValue.toFixed(3)} normalized`}>
          {parameter.displayValue
            ? `${parameter.displayValue}${parameter.label ? ` ${parameter.label}` : ''}`
            : canonicalValue.toFixed(3)}
        </output>
        {automationValue !== undefined && (
          <output aria-label={`${name} current automation value`}>
            Automation: {automationDisplay ?? automationValue.toFixed(3)}
            {parameter.label && ` ${parameter.label}`}
          </output>
        )}
      </span>
      <span className={styles.parameterTools}>
        {parameter.automatable && parameter.parameterId && (
          <button
            type="button"
            className={controls.iconButton}
            onClick={onOpenAutomation}
            aria-label={`Open ${name} automation`}
            title="Open automation"
          >
            <Icon name="curve" />
          </button>
        )}
        <button
          type="button"
          className={controls.iconButton}
          disabled={disabled}
          aria-label={`Reset ${name}`}
          title="Reset to default"
          onClick={() => {
            void commit(parameter.defaultValue);
          }}
        >
          <Icon name="undo" />
        </button>
      </span>
    </div>
  );
}

export function DeviceParameterEditor(props: DeviceParameterEditorProps) {
  const { api } = useSessionContext();
  const { details, loading, error, saving, mutate, openEditor } = useDeviceDetails(
    props.owner,
    props.deviceId,
    props.parameterValues,
    props.stateData,
  );
  return (
    <section
      className={styles.details}
      aria-label="Device parameters"
      aria-busy={loading || saving}
    >
      {loading && (
        <p className={styles.notice} role="status">
          Loading device…
        </p>
      )}
      {props.automationReadActive && (
        <p className={styles.notice}>Automation Read · stop playback to edit device state.</p>
      )}
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {details && (
        <>
          <header className={styles.detailsHeader}>
            <strong>{details.inspection.name}</strong>
            {details.inspection.capabilities.presets && (
              <label className={styles.preset}>
                <span>Preset</span>
                <select
                  className={controls.control}
                  aria-label="Preset"
                  value={details.currentPreset?.index ?? ''}
                  disabled={loading || saving || props.automationReadActive}
                  onChange={(event) => {
                    const index = Number(event.currentTarget.value);
                    void mutate(() => api.setPluginPreset(props.owner, props.deviceId, index));
                  }}
                >
                  <option value="" disabled>
                    No current preset
                  </option>
                  {details.presets.map((preset) => (
                    <option key={preset.index} value={preset.index}>
                      {preset.name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {details.inspection.capabilities.editor && (
              <button
                type="button"
                className={`${controls.smallButton} ${controls.accent}`}
                onClick={() => {
                  void openEditor();
                }}
              >
                Open Plugin Editor
              </button>
            )}
          </header>
          <DeviceSidechainEditor
            arrangement={props.arrangement}
            owner={props.owner}
            deviceId={props.deviceId}
            capabilities={details.inspection.capabilities.sidechain}
            isInstrument={props.isInstrument}
            commit={props.commit}
            onOpenAutomation={props.onOpenSidechainAutomation}
          />
          {details.parameters.length > 0 && (
            <div className={styles.parameters} role="group" aria-label="Parameters">
              {details.parameters.map((parameter) =>
                (() => {
                  const lane = props.automationLanes.find(
                    (candidate) =>
                      candidate.target.kind === 'deviceParameter' &&
                      candidate.target.deviceId === props.deviceId &&
                      candidate.target.parameterId === parameter.parameterId,
                  );
                  const staticValue = props.parameterValues[parameter.index] ?? parameter.value;
                  const currentValue = lane
                    ? automationValueAt(
                        lane.points,
                        lane.interpolation,
                        props.currentTick,
                        staticValue,
                        props.arrangement.timebase,
                      )
                    : undefined;
                  return (
                    <ParameterControl
                      key={parameter.index}
                      owner={props.owner}
                      deviceId={props.deviceId}
                      automated={Boolean(lane)}
                      onOpenAutomation={() => props.onOpenAutomation(parameter)}
                      parameter={parameter}
                      canonicalValue={staticValue}
                      automationValue={currentValue}
                      disabled={loading || saving || props.automationReadActive}
                      onCommit={(value) =>
                        mutate(() =>
                          api.setDeviceParameter(
                            props.owner,
                            props.deviceId,
                            parameter.index,
                            value,
                          ),
                        )
                      }
                    />
                  );
                })(),
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
