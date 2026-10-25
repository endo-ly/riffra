import type { KeyboardEvent } from 'react';
import type { useChannelMixControl } from './hooks/useChannelMixControl';

type MixParameter = 'gainDb' | 'pan';
type ChannelMixControl = ReturnType<typeof useChannelMixControl>;

interface ChannelMixSliderProps {
  mix: ChannelMixControl;
  parameter: MixParameter;
  value: number;
  min: number;
  max: number;
  step: number;
  ariaLabel: string;
  disabled?: boolean;
  onDoubleClick?: () => void;
}

export function ChannelMixSlider({
  mix,
  parameter,
  value,
  min,
  max,
  step,
  ariaLabel,
  disabled,
  onDoubleClick,
}: ChannelMixSliderProps) {
  const setValue = parameter === 'gainDb' ? mix.setGainDb : mix.setPan;
  const commitValue = (nextValue: number) => void mix.commit(parameter, nextValue);

  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      disabled={disabled}
      aria-label={ariaLabel}
      onPointerDown={() => mix.beginInteraction(parameter)}
      onChange={(event) => {
        const nextValue = Number(event.currentTarget.value);
        setValue(nextValue);
        mix.schedulePreview(parameter, nextValue);
      }}
      onPointerUp={(event) => commitValue(Number(event.currentTarget.value))}
      onPointerCancel={() => mix.cancel(parameter)}
      onKeyDown={(event) => handleKeyDown(event, parameter, mix)}
      onKeyUp={(event) => {
        if (isMixAdjustmentKey(event.key)) commitValue(Number(event.currentTarget.value));
      }}
      onDoubleClick={onDoubleClick}
      onBlur={(event) => commitValue(Number(event.currentTarget.value))}
    />
  );
}

function handleKeyDown(
  event: KeyboardEvent<HTMLInputElement>,
  parameter: MixParameter,
  mix: ChannelMixControl,
): void {
  if (event.key === 'Escape') mix.cancel(parameter);
  else if (isMixAdjustmentKey(event.key)) mix.beginInteraction(parameter);
}

function isMixAdjustmentKey(key: string): boolean {
  return ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(key);
}
