import type { ArrangementMutationResult, CanonicalState } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { useCallback, useEffect, useRef, useState } from 'react';

interface PreviewedValueOptions {
  /** Committed value shown while nothing is being edited. */
  value: number;
  /** Session and target being edited; a change drops the edit in progress. */
  sessionId: string;
  target: string;
  /** Reads the committed value from the latest accepted canonical state. */
  read?: (canonical: CanonicalState) => number | undefined;
  submit: (value: number) => Promise<ArrangementMutationResult>;
  /** Sends a Runtime-only value while dragging; omitted for values without a preview. */
  preview?: (value: number) => Promise<unknown>;
  disabled?: boolean;
  onError?: (message: string) => void;
}

export interface PreviewedValue {
  /** The value to show: the edited value while editing, otherwise the committed one. */
  value: number;
  begin(): void;
  change(value: number): void;
  commit(value: number): Promise<void>;
  cancel(): void;
}

interface Identity {
  sessionId: string;
  target: string;
  projectId?: string;
}

/** Coordinates one value's Runtime preview and one canonical commit. */
export function usePreviewedValue(options: PreviewedValueOptions): PreviewedValue {
  const { commit: sessionCommit, getCanonicalState } = useSessionContext();
  const [draft, setDraft] = useState(options.value);
  const canonical = useRef(options.value);
  const latest = useRef(options);
  const identity = useRef<Identity>({ sessionId: options.sessionId, target: options.target });
  const interaction = useRef(0);
  const editing = useRef(false);
  const canceled = useRef(false);
  const completed = useRef<number | null>(null);
  const pending = useRef<number | null>(null);
  const previewDirty = useRef(false);
  const timer = useRef<number | undefined>(undefined);
  const previewVersion = useRef(0);
  const previewChain = useRef<Promise<void>>(Promise.resolve());
  const disposed = useRef(false);

  latest.current = options;

  const clearTimer = useCallback(() => {
    if (timer.current !== undefined) {
      window.clearTimeout(timer.current);
      timer.current = undefined;
    }
  }, []);

  const queuePreview = useCallback((value: number) => {
    const request = ++previewVersion.current;
    previewChain.current = previewChain.current
      .catch(() => undefined)
      .then(async () => {
        const current = latest.current;
        if (
          disposed.current ||
          previewVersion.current !== request ||
          current.sessionId !== identity.current.sessionId ||
          current.target !== identity.current.target ||
          current.disabled ||
          !Number.isFinite(value) ||
          !current.preview
        )
          return;
        previewDirty.current = true;
        await current.preview(value);
      })
      .catch(() => undefined);
    return previewChain.current;
  }, []);

  const restoreRuntime = useCallback(
    (value: number) => {
      clearTimer();
      setDraft(value);
      if (previewDirty.current && latest.current.preview && !latest.current.disabled) {
        const restoreVersion = previewVersion.current + 1;
        void queuePreview(value).then(() => {
          if (previewVersion.current === restoreVersion) previewDirty.current = false;
        });
      }
    },
    [clearTimer, queuePreview],
  );

  useEffect(() => {
    const nextIdentity = { sessionId: options.sessionId, target: options.target };
    if (
      identity.current.sessionId !== nextIdentity.sessionId ||
      identity.current.target !== nextIdentity.target
    ) {
      identity.current = nextIdentity;
      interaction.current += 1;
      editing.current = false;
      canceled.current = false;
      completed.current = null;
      pending.current = null;
      previewDirty.current = false;
      clearTimer();
      previewVersion.current += 1;
      previewChain.current = Promise.resolve();
      canonical.current = options.value;
      setDraft(options.value);
      return;
    }

    const previous = canonical.current;
    canonical.current = options.value;
    if (editing.current) return;
    setDraft(options.value);
    if (previous !== options.value && previewDirty.current) restoreRuntime(options.value);
  }, [clearTimer, options.sessionId, options.target, options.value, restoreRuntime]);

  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
      clearTimer();
      previewVersion.current += 1;
    };
  }, [clearTimer]);

  const begin = useCallback(() => {
    interaction.current += 1;
    editing.current = true;
    canceled.current = false;
    completed.current = null;
    clearTimer();
    previewVersion.current += 1;
  }, [clearTimer]);

  const change = useCallback(
    (value: number) => {
      setDraft(value);
      const current = latest.current;
      if (current.disabled || !current.preview || !Number.isFinite(value)) return;
      clearTimer();
      const request = ++previewVersion.current;
      timer.current = window.setTimeout(() => {
        timer.current = undefined;
        if (previewVersion.current === request) void queuePreview(value);
      }, 40);
    },
    [clearTimer, queuePreview],
  );

  const cancel = useCallback(() => {
    canceled.current = true;
    editing.current = false;
    interaction.current += 1;
    pending.current = null;
    clearTimer();
    restoreRuntime(canonical.current);
  }, [clearTimer, restoreRuntime]);

  const commit = useCallback(
    async (value: number) => {
      if (canceled.current) {
        return;
      }
      editing.current = false;
      if (completed.current === interaction.current || pending.current === interaction.current)
        return;
      if (latest.current.disabled || !Number.isFinite(value)) {
        restoreRuntime(canonical.current);
        return;
      }

      const operationId = interaction.current;
      const operationIdentity = { ...identity.current };
      pending.current = operationId;
      clearTimer();
      if (latest.current.preview) await queuePreview(value);
      await previewChain.current.catch(() => undefined);
      if (
        disposed.current ||
        identity.current.sessionId !== operationIdentity.sessionId ||
        identity.current.target !== operationIdentity.target ||
        latest.current.sessionId !== operationIdentity.sessionId ||
        latest.current.target !== operationIdentity.target ||
        interaction.current !== operationId
      )
        return;

      const currentOptions = latest.current;
      const before = getCanonicalState();
      if (currentOptions.read) {
        if (!before || before.session.sessionId !== operationIdentity.sessionId) {
          completed.current = operationId;
          pending.current = null;
          return;
        }
        operationIdentity.projectId = before.projectId;
        const currentValue = currentOptions.read(before);
        if (currentValue === undefined) {
          completed.current = operationId;
          pending.current = null;
          return;
        }
        canonical.current = currentValue;
      } else if (before) {
        if (before.session.sessionId !== operationIdentity.sessionId) {
          completed.current = operationId;
          pending.current = null;
          return;
        }
        operationIdentity.projectId = before.projectId;
      }

      if (value === canonical.current) {
        completed.current = operationId;
        pending.current = null;
        previewDirty.current = false;
        setDraft(canonical.current);
        return;
      }

      const result = await sessionCommit(currentOptions.submit(value), {
        onError: currentOptions.onError,
      });
      if (
        disposed.current ||
        identity.current.sessionId !== operationIdentity.sessionId ||
        identity.current.target !== operationIdentity.target ||
        latest.current.sessionId !== operationIdentity.sessionId ||
        latest.current.target !== operationIdentity.target ||
        interaction.current !== operationId
      )
        return;

      const after = getCanonicalState();
      if (
        !after ||
        after.session.sessionId !== operationIdentity.sessionId ||
        (operationIdentity.projectId !== undefined &&
          after.projectId !== operationIdentity.projectId) ||
        (result && after.projectId !== result.canonical.projectId)
      ) {
        completed.current = operationId;
        pending.current = null;
        return;
      }

      const currentValue = latest.current.read?.(after);
      if (latest.current.read && currentValue === undefined) {
        completed.current = operationId;
        pending.current = null;
        previewDirty.current = false;
        return;
      }
      const committed = currentValue ?? (result ? latest.current.value : undefined);
      if (committed !== undefined) canonical.current = committed;
      if (!editing.current) setDraft(canonical.current);
      completed.current = operationId;
      pending.current = null;
      if (!result || result.projection.state === 'failed') {
        if (latest.current.preview) {
          const restoreVersion = previewVersion.current + 1;
          await queuePreview(canonical.current);
          if (previewVersion.current === restoreVersion) previewDirty.current = false;
        }
      } else {
        previewDirty.current = false;
      }
    },
    [clearTimer, getCanonicalState, queuePreview, restoreRuntime, sessionCommit],
  );

  return { value: draft, begin, change, commit, cancel };
}
