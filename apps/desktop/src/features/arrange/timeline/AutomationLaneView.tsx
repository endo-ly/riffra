import { useEffect, useRef, useState } from 'react';
import type {
  AutomationInterpolation,
  AutomationLane,
  AutomationPoint,
  AutomationTarget,
} from '@/model/domain';
import { automationTargetKey, type AutomationOption } from '../model/automation-targets';
import { isEditableTarget } from '@/features/arrange/model/interaction';
import styles from '../WorkspaceArrange.module.css';

const HEIGHT = 128;

interface AutomationLaneViewProps {
  option: AutomationOption;
  options: AutomationOption[];
  lane?: AutomationLane;
  error?: string | null;
  timelineWidth: number;
  pixelsPerTick: number;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  onTarget: (target: AutomationTarget) => void;
  onClose: () => void;
  onCommit: (points: AutomationPoint[], interpolation: AutomationInterpolation) => void;
}

export function AutomationLaneView(props: AutomationLaneViewProps) {
  const { min, max, unit: label, name, staticValue: baseValue } = props.option;
  const interpolation = props.lane?.interpolation ?? (props.option.discrete ? 'step' : 'linear');
  const cancelDrag = useRef<(() => void) | null>(null);
  useEffect(
    () => () => {
      cancelDrag.current?.();
    },
    [],
  );
  const [points, setPoints] = useState(props.lane?.points ?? []);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  useEffect(() => {
    cancelDrag.current?.();
    setPoints(props.lane?.points ?? []);
  }, [props.lane]);

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
    props.onCommit(next, interpolation);
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
    };
    const finish = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
      cancelDrag.current = null;
      props.onCommit(preview, interpolation);
    };
    const cancel = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', key);
      cancelDrag.current = null;
      setPoints(props.lane?.points ?? []);
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
          value={interpolation}
          disabled={!points.length}
          onChange={(event) =>
            props.onCommit(points, event.target.value as AutomationInterpolation)
          }
        >
          <option value="linear" disabled={props.option.discrete}>
            Linear
          </option>
          <option value="step">Step</option>
        </select>
        <span title={props.error ?? undefined}>
          {props.error ?? `Static: ${baseValue === undefined ? '—' : baseValue} ${label}`}
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
          if (isEditableTarget(event.target)) return;
          event.preventDefault();
          const next = points.filter((point) => point.id !== selectedId);
          setPoints(next);
          setSelectedId(null);
          props.onCommit(next, interpolation);
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
