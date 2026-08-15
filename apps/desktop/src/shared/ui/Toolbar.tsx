import type { ReactNode } from 'react';
import { Icon } from './primitives';
import styles from './Toolbar.module.css';

export function Toolbar(props: { label: string; children: ReactNode; trailing?: ReactNode }) {
  return (
    <header className={styles.toolbar} aria-label={props.label}>
      {props.children}
      {props.trailing !== undefined ? (
        <div className={styles.trailing}>{props.trailing}</div>
      ) : null}
    </header>
  );
}

export function ToolbarDivider() {
  return <span className={styles.divider} aria-hidden="true" />;
}

export function ToolbarSegmented<T extends string>(props: {
  label: string;
  value: T;
  options: readonly { value: T; label: string; icon?: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div className={styles.segmented} role="group" aria-label={props.label}>
      {props.options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={
            option.value === props.value
              ? `${styles.segmentedButton} ${styles.active}`
              : styles.segmentedButton
          }
          aria-pressed={option.value === props.value}
          aria-label={option.icon !== undefined ? option.label : undefined}
          title={option.label}
          onClick={() => props.onChange(option.value)}
        >
          {option.icon !== undefined ? <Icon name={option.icon} /> : option.label}
        </button>
      ))}
    </div>
  );
}

export function ToolbarTabs<T extends string>(props: {
  label: string;
  value: T;
  options: readonly { value: T; label: string; disabled?: boolean }[];
  onChange: (value: T) => void;
}) {
  return (
    <div className={styles.segmented} role="tablist" aria-label={props.label}>
      {props.options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="tab"
          aria-selected={option.value === props.value}
          className={
            option.value === props.value
              ? `${styles.segmentedButton} ${styles.active}`
              : styles.segmentedButton
          }
          disabled={option.disabled}
          onClick={() => props.onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function ToolbarButton(props: {
  icon?: string;
  ariaLabel?: string;
  disabled?: boolean;
  title?: string;
  onClick: () => void;
  children?: ReactNode;
}) {
  const iconOnly = props.children === undefined;
  return (
    <button
      type="button"
      className={iconOnly ? `${styles.control} ${styles.iconOnly}` : styles.control}
      aria-label={props.ariaLabel}
      disabled={props.disabled}
      title={props.title ?? props.ariaLabel}
      onClick={props.onClick}
    >
      {props.icon !== undefined ? <Icon name={props.icon} /> : null}
      {props.children}
    </button>
  );
}

export function ToolbarToggle(props: {
  active: boolean;
  icon?: string;
  ariaLabel?: string;
  disabled?: boolean;
  title?: string;
  onClick: () => void;
  children?: ReactNode;
}) {
  const iconOnly = props.children === undefined;
  const classes = [styles.control];
  if (iconOnly) classes.push(styles.iconOnly);
  if (props.active) classes.push(styles.active);
  return (
    <button
      type="button"
      className={classes.join(' ')}
      aria-pressed={props.active}
      aria-label={props.ariaLabel}
      disabled={props.disabled}
      title={props.title ?? props.ariaLabel}
      onClick={props.onClick}
    >
      {props.icon !== undefined ? <Icon name={props.icon} /> : null}
      {props.children}
    </button>
  );
}

export function ToolbarSelect<T extends string>(props: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <label className={styles.field}>
      <span>{props.label}</span>
      <select
        className={styles.control}
        value={props.value}
        onChange={(event) => props.onChange(event.target.value as T)}
      >
        {props.options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

export function ToolbarStepper(props: {
  ariaLabel: string;
  label?: string;
  valueText?: string;
  onStep: (direction: -1 | 1) => void;
}) {
  return (
    <div className={styles.stepper} role="group" aria-label={props.ariaLabel}>
      {props.label !== undefined ? <span>{props.label}</span> : null}
      <div className={styles.segmented}>
        <button
          type="button"
          className={styles.segmentedButton}
          aria-label={`${props.ariaLabel} out`}
          onClick={() => props.onStep(-1)}
        >
          <Icon name="zoomOut" />
        </button>
        <button
          type="button"
          className={styles.segmentedButton}
          aria-label={`${props.ariaLabel} in`}
          onClick={() => props.onStep(1)}
        >
          <Icon name="zoomIn" />
        </button>
      </div>
      {props.valueText !== undefined ? (
        <span className={styles.value}>{props.valueText}</span>
      ) : null}
    </div>
  );
}

export function ToolbarSlider(props: {
  label: string;
  ariaLabel: string;
  value: number;
  min: number;
  max: number;
  disabled?: boolean;
  onChange: (value: number) => void;
  onCommit: (value: number) => void;
}) {
  return (
    <label className={styles.field}>
      <span>{props.label}</span>
      <input
        type="range"
        aria-label={props.ariaLabel}
        min={props.min}
        max={props.max}
        value={props.value}
        disabled={props.disabled}
        onChange={(event) => props.onChange(Number(event.currentTarget.value))}
        onPointerUp={(event) => props.onCommit(Number(event.currentTarget.value))}
        onKeyUp={(event) => {
          if (event.key.startsWith('Arrow') || event.key === 'Home' || event.key === 'End') {
            props.onCommit(Number(event.currentTarget.value));
          }
        }}
      />
      <span className={styles.value}>{props.value}</span>
    </label>
  );
}
