import { useEffect, useRef, useState, type CSSProperties } from 'react';
import type { ArrangementMutationResult } from '@/model/domain';
import { formatGainDb } from '@/shared/audio/mix-format';
import controls from '@/shared/ui/controls.module.css';

const MIN_DB = -90;
const MAX_DB = 24;

interface LevelSliderProps {
  ariaLabel: string;
  initial: number;
  enabled: boolean;
  preview: (value: number) => Promise<unknown>;
  submit: (value: number) => Promise<ArrangementMutationResult>;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
}

export function LevelSlider({
  ariaLabel,
  initial,
  enabled,
  preview,
  submit,
  commit,
}: LevelSliderProps) {
  const [draft, setDraft] = useState(initial);
  const timer = useRef<number | undefined>(undefined);
  const chain = useRef(Promise.resolve());
  const version = useRef(0);
  const submitted = useRef(initial);
  useEffect(() => {
    setDraft(initial);
    submitted.current = initial;
  }, [initial]);
  useEffect(
    () => () => {
      version.current++;
      window.clearTimeout(timer.current);
    },
    [],
  );
  useEffect(() => {
    if (enabled) return;
    version.current++;
    window.clearTimeout(timer.current);
  }, [enabled]);
  const previewValue = (value: number) => {
    if (!enabled) return;
    window.clearTimeout(timer.current);
    const request = ++version.current;
    timer.current = window.setTimeout(() => {
      chain.current = chain.current
        .catch(() => undefined)
        .then(async () => {
          if (request === version.current) await preview(value);
        })
        .catch(() => undefined);
    }, 40);
  };
  const finish = (value: number) => {
    window.clearTimeout(timer.current);
    if (submitted.current === value) return;
    const request = ++version.current;
    submitted.current = value;
    void chain.current
      .catch(() => undefined)
      .then(async () => {
        if (request !== version.current) return;
        if (enabled) await preview(value).catch(() => undefined);
        if (request !== version.current) return;
        commit(
          submit(value).catch((error) => {
            if (request === version.current) {
              setDraft(initial);
              submitted.current = initial;
              previewValue(initial);
            }
            throw error;
          }),
        );
      });
  };
  const fill = {
    '--fill-start': '0%',
    '--fill-end': `${((draft - MIN_DB) / (MAX_DB - MIN_DB)) * 100}%`,
  } as CSSProperties;
  return (
    <>
      <input
        type="range"
        className={controls.fillSlider}
        style={fill}
        aria-label={ariaLabel}
        min={MIN_DB}
        max={MAX_DB}
        step={0.1}
        value={draft}
        onChange={(e) => {
          const value = Number(e.currentTarget.value);
          setDraft(value);
          previewValue(value);
        }}
        onPointerUp={(e) => finish(Number(e.currentTarget.value))}
        onKeyUp={(e) => {
          if (e.key !== 'Escape') finish(Number(e.currentTarget.value));
        }}
        onPointerCancel={() => {
          submitted.current = initial;
          setDraft(initial);
          previewValue(initial);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            submitted.current = initial;
            setDraft(initial);
            previewValue(initial);
          }
        }}
        onBlur={(e) => finish(Number(e.currentTarget.value))}
      />
      <output>{formatGainDb(draft)}</output>
    </>
  );
}
