import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AudioTakeVariant, CreativeSession, RecordingTakeRecord } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import surface from '@/shared/ui/Surface.module.css';
import { useInspectorOperation } from './useInspectorOperation';
import styles from './TakeInspector.module.css';

type RecordingSession = CreativeSession['arrangement']['recordingSessions'][number];

interface AuditionState {
  takeId: string;
  variant: AudioTakeVariant;
}

const PREVIEW_END_GRACE_MS = 250;

interface TakeInspectorProps {
  session: CreativeSession;
  selection: ArrangeSelection;
  setSession: (session: CreativeSession) => void;
  recordingActive: boolean;
  recordingCommandPending: boolean;
  onRecordAnotherTake: (recordingSessionId: string) => void | Promise<void>;
  api: ArrangeInspectorApi;
}

function passOrdinal(session: RecordingSession, arrangement: CreativeSession['arrangement']) {
  return session.passIds.reduce((latest, passId) => {
    const pass = arrangement.recordingPasses.find((item) => item.id === passId);
    return Math.max(latest, pass?.ordinal ?? -1);
  }, -1);
}

function compareRecordingSessions(
  first: RecordingSession,
  second: RecordingSession,
  arrangement: CreativeSession['arrangement'],
) {
  return (
    passOrdinal(first, arrangement) - passOrdinal(second, arrangement) ||
    first.startTick - second.startTick ||
    first.id.localeCompare(second.id)
  );
}

function compareTakes(
  first: RecordingTakeRecord,
  second: RecordingTakeRecord,
  arrangement: CreativeSession['arrangement'],
) {
  const firstPass = arrangement.recordingPasses.find((pass) => pass.id === first.passId);
  const secondPass = arrangement.recordingPasses.find((pass) => pass.id === second.passId);
  return (
    (firstPass?.ordinal ?? -1) - (secondPass?.ordinal ?? -1) ||
    first.startTick - second.startTick ||
    first.id.localeCompare(second.id)
  );
}

function sourceForVariant(take: RecordingTakeRecord, variant: AudioTakeVariant) {
  return variant === 'raw'
    ? (take.rawAudio ?? take.processedAudio)
    : (take.processedAudio ?? take.rawAudio);
}

function defaultVariantForTake(
  arrangement: CreativeSession['arrangement'],
  take: RecordingTakeRecord,
  selectedClipIds: string[],
): AudioTakeVariant {
  const selectedClip = arrangement.audioClips.find(
    (clip) => selectedClipIds.includes(clip.id) && clip.recordingTakeId === take.id,
  );
  const relatedClip =
    selectedClip ?? arrangement.audioClips.find((clip) => clip.recordingTakeId === take.id);
  if (relatedClip) return relatedClip.takeVariant;
  return take.processedAudio ? 'processed' : 'raw';
}

function sourceDurationMs(take: RecordingTakeRecord, variant: AudioTakeVariant) {
  const source = sourceForVariant(take, variant);
  if (!source || source.sampleRate <= 0) return PREVIEW_END_GRACE_MS;
  return Math.max(
    PREVIEW_END_GRACE_MS,
    Math.round(((source.sourceEndSample - source.sourceStartSample) / source.sampleRate) * 1000) +
      PREVIEW_END_GRACE_MS,
  );
}

export function TakeInspector(props: TakeInspectorProps) {
  const { arrangement } = props.session;
  const groupContext = useMemo(() => {
    const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
    const selectedAudioClip = selectedClipIds.length
      ? arrangement.audioClips.find((clip) => selectedClipIds.includes(clip.id))
      : undefined;
    const selectedMidiClip = selectedClipIds.length
      ? arrangement.midiClips.find((clip) => selectedClipIds.includes(clip.id))
      : undefined;
    const selectedTakeId = selectedAudioClip?.recordingTakeId ?? selectedMidiClip?.recordingTakeId;
    const selectedTake = selectedTakeId
      ? arrangement.takes.find((take) => take.id === selectedTakeId)
      : undefined;
    const selectedTrackId =
      props.selection.kind === 'track'
        ? props.selection.trackId
        : (selectedAudioClip?.trackId ?? selectedMidiClip?.trackId ?? selectedTake?.trackId);
    if (!selectedTrackId) return null;

    const sessions = arrangement.recordingSessions
      .filter((session) => session.trackSlots.some((slot) => slot.trackId === selectedTrackId))
      .sort((first, second) => compareRecordingSessions(first, second, arrangement));
    if (sessions.length === 0) return null;
    return {
      selectedClipIds,
      selectedTake,
      selectedTrackId,
      sessions,
    };
  }, [arrangement, props.selection]);

  const [selectedRecordingSessionId, setSelectedRecordingSessionId] = useState<string | null>(null);
  const preferredSessionId =
    groupContext?.selectedTake?.sessionId ?? groupContext?.sessions.at(-1)?.id ?? null;

  useEffect(() => {
    setSelectedRecordingSessionId((current) => {
      if (groupContext?.selectedTake?.sessionId) return groupContext.selectedTake.sessionId;
      if (current && groupContext?.sessions.some((session) => session.id === current))
        return current;
      return preferredSessionId;
    });
  }, [groupContext, preferredSessionId]);

  const recordingSession =
    groupContext?.sessions.find(
      (session) =>
        session.id === (groupContext.selectedTake?.sessionId ?? selectedRecordingSessionId),
    ) ?? groupContext?.sessions.at(-1);
  const context = useMemo(() => {
    if (!groupContext || !recordingSession) return null;
    return {
      ...groupContext,
      recordingSession,
      takes: arrangement.takes
        .filter(
          (take) =>
            take.sessionId === recordingSession.id && take.trackId === groupContext.selectedTrackId,
        )
        .sort((first, second) => compareTakes(first, second, arrangement)),
    };
  }, [arrangement, groupContext, recordingSession]);

  const [audition, setAudition] = useState<AuditionState | null>(null);
  const [sourceVariants, setSourceVariants] = useState<Record<string, AudioTakeVariant>>({});
  const auditionTimer = useRef<number | null>(null);
  const auditionRequest = useRef(0);
  const { operationMessage, runOperation } = useInspectorOperation();

  const clearAuditionTimer = useCallback(() => {
    if (auditionTimer.current === null) return;
    window.clearTimeout(auditionTimer.current);
    auditionTimer.current = null;
  }, []);

  const stopNativeAudition = useCallback(
    () =>
      Promise.all([props.api.stopTakeComparison(), props.api.stopSamplePreview()]).then(
        () => undefined,
      ),
    [props.api],
  );

  const resetAudition = useCallback(() => {
    auditionRequest.current += 1;
    clearAuditionTimer();
    setAudition(null);
  }, [clearAuditionTimer]);

  const contextKey = context
    ? `${context.recordingSession.id}:${context.selectedTrackId}`
    : 'no-take-context';
  useEffect(() => {
    resetAudition();
    setSourceVariants({});
    void stopNativeAudition().catch(() => undefined);
  }, [contextKey, resetAudition, stopNativeAudition]);

  useEffect(() => {
    if (!props.recordingActive && !props.recordingCommandPending) return;
    resetAudition();
    void stopNativeAudition().catch(() => undefined);
  }, [props.recordingActive, props.recordingCommandPending, resetAudition, stopNativeAudition]);

  useEffect(
    () => () => {
      clearAuditionTimer();
      void stopNativeAudition().catch(() => undefined);
    },
    [clearAuditionTimer, stopNativeAudition],
  );

  const scheduleAuditionEnd = useCallback(
    (take: RecordingTakeRecord, variant: AudioTakeVariant) => {
      clearAuditionTimer();
      auditionTimer.current = window.setTimeout(
        () => {
          auditionRequest.current += 1;
          setAudition((current) =>
            current?.takeId === take.id && current.variant === variant ? null : current,
          );
          auditionTimer.current = null;
        },
        sourceDurationMs(take, variant),
      );
    },
    [clearAuditionTimer],
  );

  const commit = (promise: Promise<CreativeSession>) => runOperation(promise, props.setSession);

  const preview = (take: RecordingTakeRecord) => {
    const variant =
      sourceVariants[take.id] ??
      defaultVariantForTake(arrangement, take, context?.selectedClipIds ?? []);
    const source = sourceForVariant(take, variant);
    if (!source) {
      runOperation(Promise.reject(new Error('This MIDI Take has no audio preview.')));
      return;
    }
    if (audition?.takeId === take.id) {
      resetAudition();
      runOperation(stopNativeAudition());
      return;
    }

    resetAudition();
    const requestId = auditionRequest.current;
    const operation = stopNativeAudition().then(() => {
      if (auditionRequest.current !== requestId) return null;
      if (take.rawAudio && take.processedAudio) {
        return props.api
          .startTakeComparison(take.id)
          .then((status) =>
            variant === 'processed' ? props.api.switchTakeComparisonVariant('processed') : status,
          );
      }
      return props.api.previewAsset(source.assetId, {
        startMs: (source.sourceStartSample / source.sampleRate) * 1000,
        endMs: (source.sourceEndSample / source.sampleRate) * 1000,
        looped: false,
      });
    });
    runOperation(operation, () => {
      if (auditionRequest.current !== requestId) return;
      setAudition({ takeId: take.id, variant });
      scheduleAuditionEnd(take, variant);
    });
  };

  const selectSource = (take: RecordingTakeRecord, variant: AudioTakeVariant) => {
    setSourceVariants((current) => ({ ...current, [take.id]: variant }));
    if (!take.rawAudio || !take.processedAudio || audition?.takeId !== take.id) return;
    const requestId = auditionRequest.current;
    runOperation(props.api.switchTakeComparisonVariant(variant), () => {
      if (auditionRequest.current !== requestId) return;
      setAudition({ takeId: take.id, variant });
      scheduleAuditionEnd(take, variant);
    });
  };

  if (!context || context.takes.length === 0) return null;
  const canRecordAnotherTake = !props.recordingActive && !props.recordingCommandPending;
  const startAnotherTake = () => {
    resetAudition();
    void stopNativeAudition().catch(() => undefined);
    void Promise.resolve(props.onRecordAnotherTake(context.recordingSession.id)).catch(
      (error: unknown) => runOperation(Promise.reject(error)),
    );
  };

  return (
    <section className={styles.section} aria-label="Recording takes">
      <header className={styles.sectionHeader}>
        <strong>TAKES</strong>
        <button
          className={surface.textButton}
          type="button"
          disabled={!canRecordAnotherTake}
          aria-label="Record another take"
          aria-busy={props.recordingCommandPending}
          onClick={startAnotherTake}
        >
          {props.recordingActive || props.recordingCommandPending ? 'Recording…' : 'Record another'}
        </button>
      </header>

      {context.sessions.length > 1 && (
        <label className={styles.groupSelector}>
          <span>Recording group</span>
          <select
            aria-label="Recording group"
            disabled={Boolean(context.selectedTake)}
            value={context.recordingSession.id}
            onChange={(event) => setSelectedRecordingSessionId(event.target.value)}
          >
            {context.sessions.map((session, index) => (
              <option key={session.id} value={session.id}>
                Group {index + 1}
              </option>
            ))}
          </select>
        </label>
      )}

      {context.takes.map((take, index) => {
        const active = context.recordingSession.trackSlots.some(
          (slot) => slot.trackId === take.trackId && slot.activeTakeId === take.id,
        );
        const hasPreview = Boolean(take.rawAudio || take.processedAudio);
        const hasComparison = Boolean(take.rawAudio && take.processedAudio);
        const selectedVariant =
          sourceVariants[take.id] ??
          defaultVariantForTake(arrangement, take, context.selectedClipIds);
        const isAuditioning = audition?.takeId === take.id;

        return (
          <div className={styles.takeRow} key={take.id}>
            <div className={styles.takeHeader}>
              <strong>Take {index + 1}</strong>
              <div className={styles.takeMeta}>
                {!hasPreview && <span className={styles.typeLabel}>MIDI</span>}
                {active && <span className={styles.currentBadge}>CURRENT</span>}
              </div>
            </div>

            {hasComparison && (
              <div
                className={styles.sourceBlock}
                role="group"
                aria-label={`Take ${index + 1} source`}
              >
                <span className={styles.sourceLabel}>Audio source</span>
                <div className={styles.sourceControl}>
                  <button
                    type="button"
                    aria-pressed={selectedVariant === 'raw'}
                    onClick={() => selectSource(take, 'raw')}
                  >
                    Raw
                  </button>
                  <button
                    type="button"
                    aria-pressed={selectedVariant === 'processed'}
                    onClick={() => selectSource(take, 'processed')}
                  >
                    Processed
                  </button>
                </div>
              </div>
            )}

            <div className={styles.actions}>
              {!active && (
                <button
                  className={`${styles.actionButton} ${styles.useButton}`}
                  type="button"
                  aria-label={`Use Take ${index + 1}`}
                  onClick={() =>
                    commit(props.api.activateTake(context.recordingSession.id, take.id))
                  }
                >
                  Use
                </button>
              )}
              <button
                className={`${styles.actionButton} ${styles.copyButton}`}
                type="button"
                onClick={() => commit(props.api.placeTakeAsSeparateClip(take.id))}
              >
                Place copy
              </button>
              {hasPreview && (
                <button
                  className={`${styles.actionButton} ${isAuditioning ? styles.stopButton : styles.previewButton}`}
                  type="button"
                  aria-pressed={isAuditioning}
                  onClick={() => preview(take)}
                >
                  {isAuditioning ? 'Stop' : 'Preview'}
                </button>
              )}
            </div>
          </div>
        );
      })}
      {operationMessage && (
        <p className={styles.message} role="status">
          {operationMessage}
        </p>
      )}
    </section>
  );
}
