import { useEffect, useState } from 'react';
import type { AutomationLane, AutomationParameter, AutomationPoint, Track } from '@/lib/domain';
import { isEditableTarget } from '@/lib/interaction';
import styles from './WorkspaceArrange.module.css';

const HEIGHT = 84;

interface AutomationLaneViewProps {
  track: Track;
  lane?: AutomationLane;
  parameter: AutomationParameter;
  timelineWidth: number;
  pixelsPerTick: number;
  snapTick: (tick: number, temporaryOff?: boolean) => number;
  onParameter: (parameter: AutomationParameter) => void;
  onCommit: (points: AutomationPoint[]) => void;
}

function parameterRange(parameter: AutomationParameter) {
  return parameter === 'volume'
    ? { min: -90, max: 24, label: 'dB' }
    : { min: -1, max: 1, label: '' };
}

export function AutomationLaneView(props: AutomationLaneViewProps) {
  const { min, max, label } = parameterRange(props.parameter);
  const [points, setPoints] = useState(props.lane?.points ?? []);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  useEffect(() => setPoints(props.lane?.points ?? []), [props.lane]);

  const yForValue = (value: number) => ((max - value) / (max - min)) * HEIGHT;
  const valueFromY = (clientY: number, top: number) => {
    const amount = Math.min(1, Math.max(0, (clientY - top) / HEIGHT));
    const value = max - amount * (max - min);
    return props.parameter === 'volume'
      ? Math.round(value * 10) / 10
      : Math.round(value * 100) / 100;
  };
  const baseValue = props.parameter === 'volume' ? props.track.gainDb : props.track.pan;

  const addPoint = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || event.target !== event.currentTarget) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const point: AutomationPoint = {
      id: `automation-point:${crypto.randomUUID()}`,
      tick: props.snapTick((event.clientX - bounds.left) / props.pixelsPerTick, event.altKey),
      value: valueFromY(event.clientY, bounds.top),
    };
    const next = [...points.filter((candidate) => candidate.tick !== point.tick), point].sort(
      (left, right) => left.tick - right.tick,
    );
    setPoints(next);
    setSelectedId(point.id);
    props.onCommit(next);
  };

  const movePoint = (event: React.PointerEvent<HTMLButtonElement>, pointId: string) => {
    event.preventDefault();
    event.stopPropagation();
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
      props.onCommit(preview);
    };
    const cancel = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      setPoints(props.lane?.points ?? []);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
  };

  return (
    <div className={styles.automationRow}>
      <aside className={styles.automationHeader}>
        <strong>AUTOMATION</strong>
        <span>{props.track.name}</span>
        <select
          aria-label={`${props.track.name} automation parameter`}
          value={props.parameter}
          onChange={(event) => props.onParameter(event.target.value as AutomationParameter)}
        >
          <option value="volume">Volume</option>
          <option value="pan">Pan</option>
        </select>
      </aside>
      <div
        className={styles.automationLane}
        style={{ width: props.timelineWidth }}
        tabIndex={0}
        aria-label={`${props.track.name} ${props.parameter} automation`}
        onPointerDown={addPoint}
        onKeyDown={(event) => {
          if (event.key !== 'Delete' || !selectedId) return;
          if (isEditableTarget(event.target)) return;
          event.preventDefault();
          const next = points.filter((point) => point.id !== selectedId);
          setPoints(next);
          setSelectedId(null);
          props.onCommit(next);
        }}
      >
        <span
          className={styles.automationBase}
          style={{ top: yForValue(Math.min(max, Math.max(min, baseValue))) }}
        />
        <svg width={props.timelineWidth} height={HEIGHT} aria-hidden="true">
          <polyline
            points={points
              .map((point) => `${point.tick * props.pixelsPerTick},${yForValue(point.value)}`)
              .join(' ')}
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
            aria-label={`${props.parameter} ${point.value}${label} at tick ${point.tick}`}
            title={`${point.value}${label} · Tick ${point.tick}`}
            onPointerDown={(event) => movePoint(event, point.id)}
          />
        ))}
      </div>
    </div>
  );
}
