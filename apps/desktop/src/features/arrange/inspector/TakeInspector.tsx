import { useEffect, useMemo, useState } from 'react';
import type { CreativeSession, RecordingTakeRecord } from '@/model/domain';
import type { NativeApi } from '@/native/native-api';
import type { ArrangeSelection } from '@/features/arrange/hooks/useArrangeEditor';
import { useInspectorOperation } from './useInspectorOperation';
import styles from './TakeInspector.module.css';

interface TakeInspectorProps {
  session: CreativeSession;
  selection: ArrangeSelection;
  setSession: (session: CreativeSession) => void;
  api: NativeApi;
}

export function TakeInspector(props: TakeInspectorProps) {
  const { arrangement } = props.session;
  const context = useMemo(() => {
    const selectedClipIds = props.selection.kind === 'clips' ? props.selection.clipIds : [];
    const selectedClip =
      selectedClipIds.length > 0
        ? arrangement.audioClips.find((clip) => selectedClipIds.includes(clip.id))
        : undefined;
    const selectedTake = selectedClip?.recordingTakeId
      ? arrangement.takes.find((take) => take.id === selectedClip.recordingTakeId)
      : undefined;
    const selectedTrackId =
      props.selection.kind === 'track' ? props.selection.trackId : selectedTake?.trackId;
    const recordingSession = selectedTake
      ? arrangement.recordingSessions.find((item) => item.id === selectedTake.sessionId)
      : arrangement.recordingSessions.find((item) =>
          item.trackSlots.some((slot) => slot.trackId === selectedTrackId),
        );
    if (!recordingSession || !selectedTrackId) return null;
    return {
      recordingSession,
      selectedTrackId,
      takes: arrangement.takes.filter(
        (take) => take.sessionId === recordingSession.id && take.trackId === selectedTrackId,
      ),
    };
  }, [arrangement, props.selection]);
  const [previewingTake, setPreviewingTake] = useState<string | null>(null);
  const [comparisonVariant, setComparisonVariant] = useState<'raw' | 'processed'>('raw');
  const { operationMessage, runOperation } = useInspectorOperation();

  useEffect(
    () => () => {
      void props.api.stopTakeComparison().catch(() => undefined);
    },
    [props.api],
  );

  if (!context || context.takes.length === 0) return null;
  const commit = (promise: Promise<CreativeSession>) => runOperation(promise, props.setSession);
  const preview = (take: RecordingTakeRecord, variant?: 'raw' | 'processed') => {
    const selectedVariant =
      variant ??
      arrangement.audioClips.find((clip) => clip.recordingTakeId === take.id)?.takeVariant ??
      'processed';
    const assetId =
      selectedVariant === 'raw'
        ? (take.rawAudio?.assetId ?? take.processedAudio?.assetId)
        : (take.processedAudio?.assetId ?? take.rawAudio?.assetId);
    if (!assetId) {
      runOperation(Promise.reject(new Error('This Take has no previewable audio source.')));
      return;
    }
    if (variant && take.rawAudio && take.processedAudio) {
      if (previewingTake === take.id) {
        runOperation(props.api.switchTakeComparisonVariant(selectedVariant), () => {
          setComparisonVariant(selectedVariant);
        });
      } else {
        const comparison = props.api.startTakeComparison(take.id).then(async (status) => {
          if (selectedVariant === 'processed') {
            return await props.api.switchTakeComparisonVariant('processed');
          }
          return status;
        });
        runOperation(comparison, () => {
          setPreviewingTake(take.id);
          setComparisonVariant(selectedVariant);
        });
      }
    } else {
      runOperation(props.api.previewAsset(assetId, { looped: false }));
    }
  };

  return (
    <section className={styles.section} aria-label="Recording takes">
      <header className={styles.sectionHeader}>
        <strong>TAKES</strong>
      </header>
      {context.takes.map((take, index) => {
        const active = context.recordingSession.trackSlots.some(
          (slot) => slot.trackId === take.trackId && slot.activeTakeId === take.id,
        );
        return (
          <div className={styles.takeRow} key={take.id}>
            <div className={styles.takeHeader}>
              <strong>Take {index + 1}</strong>
            </div>
            <div className={styles.actions}>
              <button type="button" onClick={() => preview(take)}>
                Preview
              </button>
              {!active && (
                <button
                  type="button"
                  onClick={() =>
                    commit(props.api.activateTake(context.recordingSession.id, take.id))
                  }
                >
                  Use
                </button>
              )}
              <button
                type="button"
                onClick={() => commit(props.api.placeTakeAsSeparateClip(take.id))}
              >
                Place copy
              </button>
            </div>
            {take.rawAudio && take.processedAudio && (
              <div
                className={styles.comparison}
                role="group"
                aria-label={`Compare Take ${index + 1}`}
              >
                <button
                  type="button"
                  aria-pressed={previewingTake === take.id && comparisonVariant === 'raw'}
                  onClick={() => preview(take, 'raw')}
                >
                  A · RAW
                </button>
                <button
                  type="button"
                  aria-pressed={previewingTake === take.id && comparisonVariant === 'processed'}
                  onClick={() => preview(take, 'processed')}
                >
                  B · PROCESSED
                </button>
              </div>
            )}
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
