import { useEffect, useRef, useState } from 'react';
import type {
  AutomationLane,
  CanonicalState,
  DeviceParameterInfo,
  EffectOwner,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import { useDeviceDetails } from './useDeviceDetails';
import { automationValueAt } from '@/features/arrange/model/automation-value';
import styles from './Devices.module.css';

interface DeviceParameterEditorProps {
  api: ArrangeWorkspaceApi;
  owner: EffectOwner;
  deviceId: string;
  stateData?: string;
  parameterValues: number[];
  automationLanes: AutomationLane[];
  automationReadActive: boolean;
  currentTick: number;
  onOpenAutomation: (parameter: DeviceParameterInfo) => void;
  applyCanonicalState: (state: CanonicalState) => boolean;
}

function ParameterControl({
  parameter,
  canonicalValue,
  disabled,
  onCommit,
  automated,
  onOpenAutomation,
  automationValue,
  formatAutomationValue,
}: {
  parameter: DeviceParameterInfo;
  canonicalValue: number;
  disabled: boolean;
  onCommit: (value: number) => Promise<boolean>;
  automated: boolean;
  onOpenAutomation: () => void;
  automationValue?: number;
  formatAutomationValue: (value: number) => Promise<string>;
}) {
  const [draft, setDraft] = useState(parameter.value);
  const [automationDisplay, setAutomationDisplay] = useState<string | null>(null);
  const submitted = useRef(parameter.value);
  useEffect(() => {
    setDraft(parameter.value);
    submitted.current = parameter.value;
  }, [parameter]);
  useEffect(() => {
    let current = true;
    if (automationValue === undefined) {
      setAutomationDisplay(null);
      return;
    }
    void formatAutomationValue(automationValue).then(
      (display) => {
        if (current) setAutomationDisplay(display);
      },
      () => {
        if (current) setAutomationDisplay(null);
      },
    );
    return () => {
      current = false;
    };
  }, [automationValue, formatAutomationValue]);
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
    <div className={styles.parameter}>
      <label htmlFor={`parameter-${parameter.index}`}>
        {name} {automated ? '· AUTO' : ''}
      </label>
      {parameter.automatable && parameter.parameterId && (
        <button type="button" onClick={onOpenAutomation} aria-label={`Open ${name} automation`}>
          Automation
        </button>
      )}
      {parameter.choices.length >= 2 ? (
        <select
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
      <output>
        Static:{' '}
        <span>
          {parameter.displayValue
            ? `${parameter.displayValue}${parameter.label ? ` ${parameter.label}` : ''}`
            : `${canonicalValue.toFixed(3)} normalized`}
        </span>{' '}
        ({canonicalValue.toFixed(3)} normalized)
      </output>
      {automationValue !== undefined && (
        <output aria-label={`${name} current automation value`}>
          Automation: {automationDisplay ?? automationValue.toFixed(3)}
          {parameter.label && ` ${parameter.label}`}
        </output>
      )}
      <button
        type="button"
        disabled={disabled}
        aria-label={`Reset ${name}`}
        onClick={() => {
          void commit(parameter.defaultValue);
        }}
      >
        Default
      </button>
    </div>
  );
}

export function DeviceParameterEditor(props: DeviceParameterEditorProps) {
  const { details, loading, error, saving, mutate, openEditor } = useDeviceDetails(
    props.api,
    props.owner,
    props.deviceId,
    props.parameterValues,
    props.stateData,
    props.applyCanonicalState,
  );
  return (
    <section
      className={styles.details}
      aria-label="Device parameters"
      aria-busy={loading || saving}
    >
      {loading && <p role="status">Loading device…</p>}
      {props.automationReadActive && <p>Automation Read · stop playback to edit device state.</p>}
      {error && <p role="alert">{error}</p>}
      {details && (
        <>
          <div className={styles.actions}>
            <strong>{details.inspection.name}</strong>
            {details.inspection.capabilities.presets && (
              <label>
                Preset{' '}
                <select
                  aria-label="Preset"
                  value={details.currentPreset?.index ?? ''}
                  disabled={loading || saving || props.automationReadActive}
                  onChange={(event) => {
                    const index = Number(event.currentTarget.value);
                    void mutate(() =>
                      props.api.setPluginPreset(props.owner, props.deviceId, index),
                    );
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
                onClick={() => {
                  void openEditor();
                }}
              >
                Open Plugin Editor
              </button>
            )}
          </div>
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
                ? automationValueAt(lane.points, lane.interpolation, props.currentTick, staticValue)
                : undefined;
              return (
                <ParameterControl
                  key={parameter.index}
                  automated={Boolean(lane)}
                  onOpenAutomation={() => props.onOpenAutomation(parameter)}
                  parameter={parameter}
                  canonicalValue={staticValue}
                  automationValue={currentValue}
                  formatAutomationValue={(value) =>
                    props.api.formatDeviceParameterValue(
                      props.owner,
                      props.deviceId,
                      parameter.index,
                      value,
                    )
                  }
                  disabled={loading || saving || props.automationReadActive}
                  onCommit={(value) =>
                    mutate(() =>
                      props.api.setDeviceParameter(
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
        </>
      )}
    </section>
  );
}
