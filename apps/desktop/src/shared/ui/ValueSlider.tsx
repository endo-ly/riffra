import { useMemo, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react';
import type { MixRange } from '@/model/mix-range';
import type { PreviewedValue } from '@/shared/session/usePreviewedValue';
import controls from './controls.module.css';

interface ValueSliderProps {
  control: PreviewedValue;
  range: MixRange;
  ariaLabel: string;
  disabled?: boolean;
  /** Value the fill grows from; omitted for sliders drawn without a fill. */
  fillOrigin?: number;
  orientation?: 'horizontal' | 'vertical';
  className?: string;
}

const ADJUSTMENT_KEYS = new Set([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Home',
  'End',
  'PageUp',
  'PageDown',
]);

export function ValueSlider({
  control,
  range,
  ariaLabel,
  disabled,
  fillOrigin,
  orientation = 'horizontal',
  className,
}: ValueSliderProps) {
  const style = useMemo<CSSProperties | undefined>(() => {
    if (fillOrigin === undefined) return undefined;
    const percent = (value: number) => ((value - range.min) / (range.max - range.min)) * 100;
    return {
      '--fill-start': `${Math.min(percent(control.value), percent(fillOrigin))}%`,
      '--fill-end': `${Math.max(percent(control.value), percent(fillOrigin))}%`,
      '--fill-direction': orientation === 'vertical' ? 'to top' : 'to right',
    } as CSSProperties;
  }, [control.value, fillOrigin, orientation, range.max, range.min]);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      control.cancel();
    } else if (ADJUSTMENT_KEYS.has(event.key)) {
      control.begin();
    }
  };

  const onKeyUp = (event: KeyboardEvent<HTMLInputElement>) => {
    if (ADJUSTMENT_KEYS.has(event.key)) void control.commit(Number(event.currentTarget.value));
  };

  const onPointerUp = (event: PointerEvent<HTMLInputElement>) => {
    void control.commit(Number(event.currentTarget.value));
  };

  return (
    <input
      type="range"
      className={className ?? (fillOrigin === undefined ? undefined : controls.fillSlider)}
      style={style}
      min={range.min}
      max={range.max}
      step={range.step}
      value={control.value}
      disabled={disabled}
      aria-label={ariaLabel}
      onPointerDown={control.begin}
      onChange={(event) => control.change(Number(event.currentTarget.value))}
      onPointerUp={onPointerUp}
      onPointerCancel={control.cancel}
      onKeyDown={onKeyDown}
      onKeyUp={onKeyUp}
      onBlur={(event) => void control.commit(Number(event.currentTarget.value))}
      onDoubleClick={() => {
        control.begin();
        control.change(range.defaultValue);
        void control.commit(range.defaultValue);
      }}
    />
  );
}
