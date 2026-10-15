import { useEffect, useRef, useState } from 'react';

const FORMAT_INTERVAL_MS = 250;
const MAX_CACHED_VALUES = 64;

type Formatter = (value: number) => Promise<string>;

export function useAutomationValueDisplay(
  value: number | undefined,
  formatValue: Formatter | undefined,
): string | null {
  const [display, setDisplay] = useState<{ value: number; text: string } | null>(null);
  const latest = useRef({ value, formatValue });
  latest.current = { value, formatValue };
  const caches = useRef(new WeakMap<Formatter, Map<number, string>>());
  const lastRequestAt = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  const schedule = useRef<() => void>(() => undefined);

  schedule.current = () => {
    if (!mounted.current || timer.current !== null || inFlight.current) return;
    const current = latest.current;
    if (current.value === undefined || !current.formatValue) return;
    const cache = caches.current.get(current.formatValue);
    const cached = cache?.get(current.value);
    if (cached !== undefined) {
      if (mounted.current) setDisplay({ value: current.value, text: cached });
      return;
    }

    const delay =
      lastRequestAt.current === 0
        ? 0
        : Math.max(0, FORMAT_INTERVAL_MS - (Date.now() - lastRequestAt.current));
    timer.current = setTimeout(() => {
      timer.current = null;
      const request = latest.current;
      if (request.value === undefined || !request.formatValue) return;
      const formatter = request.formatValue;
      const requestedValue = request.value;
      const requestCache = caches.current.get(formatter);
      const requestCached = requestCache?.get(requestedValue);
      if (requestCached !== undefined) {
        if (mounted.current) setDisplay({ value: requestedValue, text: requestCached });
        return;
      }

      inFlight.current = true;
      lastRequestAt.current = Date.now();
      void formatter(requestedValue)
        .then(
          (formatted) => {
            let values = caches.current.get(formatter);
            if (!values) {
              values = new Map();
              caches.current.set(formatter, values);
            }
            values.set(requestedValue, formatted);
            if (values.size > MAX_CACHED_VALUES)
              values.delete(values.keys().next().value as number);
            if (
              mounted.current &&
              latest.current.value === requestedValue &&
              latest.current.formatValue === formatter
            )
              setDisplay({ value: requestedValue, text: formatted });
          },
          () => {
            if (
              mounted.current &&
              latest.current.value === requestedValue &&
              latest.current.formatValue === formatter
            )
              setDisplay(null);
          },
        )
        .finally(() => {
          inFlight.current = false;
          schedule.current();
        });
    }, delay);
  };

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
    };
  }, []);

  useEffect(() => {
    if (value === undefined || !formatValue) {
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
      setDisplay(null);
      return;
    }
    const cached = caches.current.get(formatValue)?.get(value);
    if (cached !== undefined) {
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
      setDisplay({ value, text: cached });
      return;
    }
    setDisplay(null);
    schedule.current();
  }, [value, formatValue]);

  return display && display.value === value ? display.text : null;
}
