import { useSessionContext } from '@/shared/session/session-context';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { Track, MixBus } from '@/model/generated';
type MixParameter = 'gainDb' | 'pan';
const mixParameters: readonly MixParameter[] = ['gainDb', 'pan'];
interface PendingCommit {
  interactionId: number;
  value: number;
}
interface UseChannelMixControlOptions {
  sessionId: string;
  channel: Track | MixBus;
  onError?: (message: string) => void;
  disabled?: boolean;
}
/** Coordinates one channel's transient preview and one-shot canonical commit. */
export function useChannelMixControl({
  sessionId,
  channel,
  onError,
  disabled = false,
}: UseChannelMixControlOptions) {
  const { api, commit: sessionCommit, getCanonicalState } = useSessionContext();
  const [gainDb, setGainDb] = useState(channel.gainDb);
  const [pan, setPan] = useState(channel.pan);
  const canonical = useRef({ gainDb: channel.gainDb, pan: channel.pan });
  const previewTimers = useRef<Partial<Record<MixParameter, number>>>({});
  const previewVersions = useRef<Record<MixParameter, number>>({ gainDb: 0, pan: 0 });
  const previewChain = useRef<Promise<void>>(Promise.resolve());
  const pendingCommit = useRef<Partial<Record<MixParameter, PendingCommit>>>({});
  const editing = useRef<Partial<Record<MixParameter, boolean>>>({});
  const interactionIds = useRef<Record<MixParameter, number>>({ gainDb: 0, pan: 0 });
  const disposed = useRef(false);
  const cancelled = useRef<Partial<Record<MixParameter, boolean>>>({});
  const activeSessionId = useRef(sessionId);
  const activeChannelId = useRef(channel.id);
  const clearPreviewTimer = useCallback((parameter: MixParameter) => {
    const timer = previewTimers.current[parameter];
    if (timer !== undefined) {
      window.clearTimeout(timer);
      delete previewTimers.current[parameter];
    }
  }, []);
  const invalidatePreview = useCallback((parameter: MixParameter) => {
    previewVersions.current[parameter] += 1;
    return previewVersions.current[parameter];
  }, []);
  const queuePreview = useCallback(
    (parameter: MixParameter, value: number, previewVersion: number) => {
      previewChain.current = previewChain.current
        .catch(() => undefined)
        .then(() => {
          if (disposed.current || previewVersions.current[parameter] !== previewVersion) return;
          return api.previewChannelMix(
            {
              kind: channel.kind === 'group' || channel.kind === 'return' ? 'bus' : 'track',
              id: channel.id,
            },
            { [parameter]: value },
          );
        })
        .catch(() => undefined);
    },
    [api, channel.id, channel.kind],
  );
  const setDraft = useCallback((parameter: MixParameter, value: number) => {
    if (parameter === 'gainDb') setGainDb(value);
    else setPan(value);
  }, []);
  const restoreCanonicalRuntime = useCallback(
    (parameter: MixParameter) => {
      const value = canonical.current[parameter];
      setDraft(parameter, value);
      clearPreviewTimer(parameter);
      const previewVersion = invalidatePreview(parameter);
      queuePreview(parameter, value, previewVersion);
    },
    [clearPreviewTimer, invalidatePreview, queuePreview, setDraft],
  );
  useEffect(() => {
    const sessionChanged = activeSessionId.current !== sessionId;
    const trackChanged = activeChannelId.current !== channel.id;
    if (sessionChanged || trackChanged) {
      activeSessionId.current = sessionId;
      activeChannelId.current = channel.id;
      cancelled.current = {};
      editing.current = {};
      mixParameters.forEach((parameter) => {
        interactionIds.current[parameter] += 1;
      });
      mixParameters.forEach(clearPreviewTimer);
      mixParameters.forEach(invalidatePreview);
      previewChain.current = Promise.resolve();
      pendingCommit.current = {};
    }
    const nextValues = { gainDb: channel.gainDb, pan: channel.pan };
    mixParameters.forEach((parameter) => {
      const previousValue = canonical.current[parameter];
      const nextValue = nextValues[parameter];
      canonical.current[parameter] = nextValue;
      if (editing.current[parameter]) return;
      setDraft(parameter, nextValue);
      if (!sessionChanged && !trackChanged && previousValue !== nextValue) {
        clearPreviewTimer(parameter);
        const previewVersion = invalidatePreview(parameter);
        queuePreview(parameter, nextValue, previewVersion);
      }
    });
  }, [
    clearPreviewTimer,
    invalidatePreview,
    queuePreview,
    sessionId,
    setDraft,
    channel.gainDb,
    channel.id,
    channel.pan,
  ]);
  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
      mixParameters.forEach(clearPreviewTimer);
    };
  }, [clearPreviewTimer]);
  const schedulePreview = useCallback(
    (parameter: MixParameter, value: number) => {
      if (disabled || !Number.isFinite(value)) return;
      clearPreviewTimer(parameter);
      const previewVersion = invalidatePreview(parameter);
      previewTimers.current[parameter] = window.setTimeout(() => {
        delete previewTimers.current[parameter];
        if (disposed.current || previewVersions.current[parameter] !== previewVersion) return;
        queuePreview(parameter, value, previewVersion);
      }, 40);
    },
    [clearPreviewTimer, disabled, invalidatePreview, queuePreview],
  );
  const commit = useCallback(
    async (parameter: MixParameter, value: number) => {
      if (cancelled.current[parameter]) {
        delete cancelled.current[parameter];
        editing.current[parameter] = false;
        return;
      }
      editing.current[parameter] = false;
      if (disabled || !Number.isFinite(value)) {
        restoreCanonicalRuntime(parameter);
        return;
      }
      const currentInteractionId = interactionIds.current[parameter];
      if (pendingCommit.current[parameter]?.interactionId === currentInteractionId) return;
      pendingCommit.current[parameter] = { interactionId: currentInteractionId, value };
      const patch = { [parameter]: value } as {
        gainDb?: number;
        pan?: number;
      };
      try {
        clearPreviewTimer(parameter);
        const previewVersion = invalidatePreview(parameter);
        queuePreview(parameter, value, previewVersion);
        await previewChain.current.catch(() => undefined);
        if (disposed.current) return;
        const before = getCanonicalState();
        if (before && before.session.sessionId !== sessionId) return;
        if (before?.session.sessionId === sessionId) {
          const currentChannel = (
            channel.kind === 'group' || channel.kind === 'return'
              ? before.session.arrangement.mixer.buses
              : before.session.arrangement.tracks
          ).find((item) => item.id === channel.id);
          if (currentChannel)
            canonical.current = { gainDb: currentChannel.gainDb, pan: currentChannel.pan };
        }
        if (value === canonical.current[parameter]) return;
        const result = await sessionCommit(
          channel.kind === 'group' || channel.kind === 'return'
            ? api.updateMixBus(channel.id, patch)
            : api.updateTrack(channel.id, patch),
          { onError },
        );
        if (
          disposed.current ||
          activeSessionId.current !== sessionId ||
          activeChannelId.current !== channel.id ||
          interactionIds.current[parameter] !== currentInteractionId
        )
          return;
        const current = getCanonicalState();
        if (
          !current ||
          current.session.sessionId !== sessionId ||
          (before && current.projectId !== before.projectId) ||
          (result && current.projectId !== result.canonical.projectId)
        )
          return;
        const committed = (
          channel.kind === 'group' || channel.kind === 'return'
            ? current.session.arrangement.mixer.buses
            : current.session.arrangement.tracks
        ).find((candidate) => candidate.id === channel.id);
        if (!committed) return;
        canonical.current = { gainDb: committed.gainDb, pan: committed.pan };
        if (!editing.current.gainDb) setGainDb(committed.gainDb);
        if (!editing.current.pan) setPan(committed.pan);
        if (!result || result.projection.state === 'failed') {
          restoreCanonicalRuntime(parameter);
        }
      } finally {
        const pending = pendingCommit.current[parameter];
        if (pending?.interactionId === currentInteractionId && pending.value === value)
          delete pendingCommit.current[parameter];
      }
    },
    [
      api,
      sessionCommit,
      getCanonicalState,
      sessionId,
      clearPreviewTimer,
      disabled,
      invalidatePreview,
      onError,
      queuePreview,
      restoreCanonicalRuntime,
      channel.id,
      channel.kind,
    ],
  );
  const cancel = useCallback(
    (parameter: MixParameter) => {
      cancelled.current[parameter] = true;
      editing.current[parameter] = false;
      interactionIds.current[parameter] += 1;
      clearPreviewTimer(parameter);
      restoreCanonicalRuntime(parameter);
    },
    [clearPreviewTimer, restoreCanonicalRuntime],
  );
  const beginInteraction = useCallback(
    (parameter: MixParameter) => {
      interactionIds.current[parameter] += 1;
      cancelled.current[parameter] = false;
      editing.current[parameter] = true;
      invalidatePreview(parameter);
    },
    [invalidatePreview],
  );
  return {
    gainDb,
    pan,
    setGainDb,
    setPan,
    beginInteraction,
    schedulePreview,
    commit,
    cancel,
  };
}
