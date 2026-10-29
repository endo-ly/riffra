import { secondsToTicks, ticksToSeconds } from '@/model/timebase';
import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import type {
  AutomationInterpolation,
  AutomationLane,
  AutomationPoint,
  AutomationTarget,
  ProjectTimebase,
} from '@/model/generated';
import { automationTargetKey, automationValueAt, type AutomationOption } from '@/model/automation';
import { isEditableTypingTarget } from '@/shared/input';
import { useAutomationValueDisplay } from '@/shared/useAutomationValueDisplay';
import styles from './Timeline.module.css';

const HEIGHT = 128;

interface AutomationLaneViewProps {
  timebase: ProjectTimebase;
  option: AutomationOption;
  options: AutomationOption[];
  lane?: AutomationLane;
  error?: string | null;
  timelineWidth: number;
  pixelsPerTick: number;
  playheadTick: number;
  playheadTickRef: MutableRefObject<number>;
  previewScope: string;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  onTarget: (target: AutomationTarget) => void;
  onClose: () => void;
  onPreview: (value: number | null, token: number) => void | Promise<void>;
  formatValue?: (value: number) => Promise<string>;
  onCommit: (
    points: AutomationPoint[],
    interpolation: AutomationInterpolation,
  ) => void | Promise<unknown>;
}

export function AutomationLaneView(props: AutomationLaneViewProps) {
  const { min, max, unit: label, name, staticValue: baseValue } = props.option;
  const { formatValue, previewScope } = props;
  const interpolation = props.lane?.interpolation ?? (props.option.discrete ? 'step' : 'linear');
  const cancelDrag = useRef<(() => void) | null>(null);
  const pendingPreview = useRef<{
    value: number | null;
    token: number;
    onPreview: AutomationLaneViewProps['onPreview'];
  } | null>(null);
  const previewDraining = useRef(false);
  const drainPreview = useRef<() => void>(() => undefined);
  drainPreview.current = () => {
    if (previewDraining.current) return;
    previewDraining.current = true;
    void (async () => {
      while (pendingPreview.current) {
        const request = pendingPreview.current;
        pendingPreview.current = null;
        try {
          await request.onPreview(request.value, request.token);
        } catch {
          // A failed preview must not block a newer value or the final clear.
        }
      }
      previewDraining.current = false;
      if (pendingPreview.current) drainPreview.current();
    })();
  };
  const queuePreview = (value: number | null, token: number) => {
    pendingPreview.current = { value, token, onPreview: props.onPreview };
    drainPreview.current();
  };
  useEffect(() => {
    return () => cancelDrag.current?.();
  }, [previewScope]);
  const [points, setPoints] = useState(props.lane?.points ?? []);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  useEffect(() => {
    cancelDrag.current?.();
    setPoints(props.lane?.points ?? []);
  }, [props.lane]);

  const effectiveValue =
    baseValue === undefined
      ? undefined
      : automationValueAt(points, interpolation, props.playheadTick, baseValue, props.timebase);
  const effectiveText = useAutomationValueDisplay(effectiveValue, formatValue);

  const yForValue = (value: number) => ((max - value) / (max - min)) * HEIGHT;
  const valueFromY = (clientY: number, top: number) => {
    const amount = Math.min(1, Math.max(0, (clientY - top) / HEIGHT));
    const value = max - amount * (max - min);
    if (props.option.discrete && (props.option.steps ?? 0) >= 2) {
      const steps = props.option.steps! - 1;
      return Math.round(value * steps) / steps;
    }
    return label === 'dB' ? Math.round(value * 10) / 10 : Math.round(value * 100) / 100;
  };

  const addPoint = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || event.target !== event.currentTarget) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const tick = props.snapTick((event.clientX - bounds.left) / props.pixelsPerTick, event.altKey);
    const point: AutomationPoint = {
      id:
        points.find((point) => point.tick === tick)?.id ??
        `automation-point:${crypto.randomUUID()}`,
      tick,
      value: valueFromY(event.clientY, bounds.top),
    };
    const next = [...points.filter((candidate) => candidate.tick !== point.tick), point].sort(
      (left, right) => left.tick - right.tick,
    );
    setPoints(next);
    setSelectedId(point.id);
    void props.onCommit(next, interpolation);
  };

  const movePoint = (event: React.PointerEvent<HTMLButtonElement>, pointId: string) => {
    event.preventDefault();
    event.stopPropagation();
    cancelDrag.current?.();
    setSelectedId(pointId);
    event.currentTarget.parentElement?.focus();
    const lane = event.currentTarget.parentElement;
    if (!lane) return;
    const bounds = lane.getBoundingClientRect();
    let preview = points;
    const previewToken = crypto.getRandomValues(new Uint32Array(1))[0] || 1;
    const move = (pointer: PointerEvent) => {
      const ordered = [...preview].sort((left, right) => left.tick - right.tick);
      const index = ordered.findIndex((point) => point.id === pointId);
      if (index < 0) return;
      const previousTick = index > 0 ? ordered[index - 1].tick + 1 : 0;
      const nextTick =
        index + 1 < ordered.length ? ordered[index + 1].tick - 1 : Number.MAX_SAFE_INTEGER;
      const tick = Math.min(
        nextTick,
        Math.max(
          previousTick,
          props.snapTick((pointer.clientX - bounds.left) / props.pixelsPerTick, pointer.altKey),
        ),
      );
      preview = ordered.map((point) =>
        point.id === pointId
          ? { ...point, tick, value: valueFromY(pointer.clientY, bounds.top) }
          : point,
      );
      setPoints(preview);
      queuePreview(
        automationValueAt(
          preview,
          interpolation,
          props.playheadTickRef.current,
          baseValue ?? 0,
          props.timebase,
        ),
        previewToken,
      );
    };
    const finish = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
      cancelDrag.current = null;
      void Promise.resolve(props.onCommit(preview, interpolation)).finally(() =>
        queuePreview(null, previewToken),
      );
    };
    const cancel = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
      cancelDrag.current = null;
      setPoints(props.lane?.points ?? []);
      queuePreview(null, previewToken);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        cancel();
      }
    };
    cancelDrag.current = cancel;
    window.addEventListener('keydown', key);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
  };

  return (
    <div className={styles.automationRow}>
      <aside className={styles.automationHeader}>
        <strong>AUTOMATION · READ</strong>
        <button type="button" onClick={props.onClose} aria-label={`Close ${name} automation`}>
          ×
        </button>
        <select
          aria-label="Automation target"
          value={automationTargetKey(props.option.target)}
          onChange={(event) => {
            const selected = props.options.find(
              (option) => automationTargetKey(option.target) === event.target.value,
            );
            if (selected) props.onTarget(selected.target);
          }}
        >
          {props.options.map((option) => (
            <option
              key={automationTargetKey(option.target)}
              value={automationTargetKey(option.target)}
            >
              {option.name}
            </option>
          ))}
        </select>
        <select
          aria-label="Automation interpolation"
          value={typeof interpolation === 'string' ? interpolation : 'linearAmplitude'}
          disabled={!points.length}
          onChange={(event) =>
            void props.onCommit(points, event.target.value as AutomationInterpolation)
          }
        >
          <option value="linear" disabled={props.option.discrete}>
            Linear
          </option>
          <option value="linearAmplitude" disabled>
            Amplitude transition
          </option>
          <option value="step">Step</option>
        </select>
        <span title={props.error ?? undefined}>
          {props.error ?? `Static: ${baseValue === undefined ? '—' : baseValue} ${label}`}
          {effectiveValue !== undefined && (
            <small> Current: {effectiveText ?? `${effectiveValue.toFixed(3)} ${label}`}</small>
          )}
        </span>
      </aside>
      <div
        className={styles.automationLane}
        style={{ width: props.timelineWidth }}
        tabIndex={0}
        aria-label={`${name} automation`}
        onPointerDown={addPoint}
        onKeyDown={(event) => {
          if (event.key !== 'Delete' || !selectedId) return;
          if (isEditableTypingTarget(event.target)) return;
          event.preventDefault();
          const next = points.filter((point) => point.id !== selectedId);
          setPoints(next);
          setSelectedId(null);
          void props.onCommit(next, interpolation);
        }}
      >
        {baseValue !== undefined && (
          <span
            className={styles.automationBase}
            style={{ top: yForValue(Math.min(max, Math.max(min, baseValue))) }}
          />
        )}
        <svg width={props.timelineWidth} height={HEIGHT} aria-hidden="true">
          <polyline
            points={
              points.length
                ? [
                    `0,${yForValue(points[0].value)}`,
                    ...points.flatMap((point, index) => {
                      const x = point.tick * props.pixelsPerTick;
                      const current = `${x},${yForValue(point.value)}`;
                      if (typeof interpolation === 'object' && index > 0) {
                        const start = ticksToSeconds(point.tick, props.timebase);
                        const end = Math.min(
                          start + interpolation.linearAmplitude.durationSeconds,
                          points[index + 1]
                            ? ticksToSeconds(points[index + 1].tick, props.timebase)
                            : Infinity,
                        );
                        return Array.from({ length: 9 }, (_, step) => {
                          const tick = secondsToTicks(
                            start + ((end - start) * step) / 8,
                            props.timebase,
                          );
                          const value = automationValueAt(
                            points,
                            interpolation,
                            tick,
                            baseValue ?? 0,
                            props.timebase,
                          );
                          return `${tick * props.pixelsPerTick},${yForValue(value)}`;
                        });
                      }
                      return interpolation === 'step' && index > 0
                        ? [`${x},${yForValue(points[index - 1].value)}`, current]
                        : [current];
                    }),
                    `${props.timelineWidth},${yForValue(points.at(-1)!.value)}`,
                  ].join(' ')
                : ''
            }
          />
        </svg>
        {points.map((point) => (
          <button
            key={point.id}
            className={
              selectedId === point.id ? styles.automationPointSelected : styles.automationPoint
            }
            style={{
              left: point.tick * props.pixelsPerTick,
              top: yForValue(point.value),
            }}
            aria-label={`${name} ${point.value}${label} at tick ${point.tick}`}
            title={`${point.value}${label} · Tick ${point.tick}`}
            onPointerDown={(event) => movePoint(event, point.id)}
          />
        ))}
      </div>
    </div>
  );
}
