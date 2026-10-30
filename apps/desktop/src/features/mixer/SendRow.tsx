import type { ReactNode } from 'react';
import type { SendPosition } from '@/model/generated';
import { MIX_GAIN_DB } from '@/model/mix-range';
import { formatGainDb } from '@/shared/audio/mix-format';
import type { PreviewedValue } from '@/shared/session/usePreviewedValue';
import { Icon } from '@/shared/ui/primitives';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import controls from '@/shared/ui/controls.module.css';
import styles from './SendRow.module.css';

interface SendRowProps {
  /** Names the row for assistive technology, e.g. `Plate Reverb send`. */
  label: string;
  /** Where the signal goes or comes from: a name or a picker. */
  endpoint: ReactNode;
  level: PreviewedValue;
  position: SendPosition;
  enabled: boolean;
  removeLabel: string;
  onPositionChange: (position: SendPosition) => void;
  onEnabledChange: (enabled: boolean) => void;
  onOpenAutomation: () => void;
  onRemove: () => void;
}

const POSITIONS: readonly [SendPosition, string, string][] = [
  ['preFader', 'Pre', 'Pre-fader'],
  ['postFader', 'Post', 'Post-fader'],
];

/** One parallel signal on a single line: switch, endpoint, level, tap point and actions. */
export function SendRow(props: SendRowProps) {
  return (
    <div className={styles.row} role="group" aria-label={props.label} data-enabled={props.enabled}>
      <input
        type="checkbox"
        role="switch"
        className={controls.switch}
        aria-label="Enabled"
        title={props.enabled ? 'Enabled' : 'Disabled'}
        checked={props.enabled}
        onChange={(event) => props.onEnabledChange(event.currentTarget.checked)}
      />
      <div className={styles.endpoint}>{props.endpoint}</div>
      <ValueSlider
        control={props.level}
        range={MIX_GAIN_DB}
        fillOrigin={MIX_GAIN_DB.min}
        ariaLabel={`${props.label} level`}
        disabled={!props.enabled}
      />
      <output>{formatGainDb(props.level.value)}</output>
      <div className={controls.segmented} role="group" aria-label="Position">
        {POSITIONS.map(([position, label, title]) => (
          <button
            key={position}
            type="button"
            title={title}
            aria-pressed={props.position === position}
            onClick={() => {
              if (props.position !== position) props.onPositionChange(position);
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <button
        type="button"
        className={controls.iconButton}
        aria-label="Automation"
        title="Open level automation"
        onClick={props.onOpenAutomation}
      >
        <Icon name="curve" />
      </button>
      <button
        type="button"
        className={`${controls.iconButton} ${controls.danger}`}
        aria-label={props.removeLabel}
        title={props.removeLabel}
        onClick={props.onRemove}
      >
        <Icon name="trash" />
      </button>
    </div>
  );
}
