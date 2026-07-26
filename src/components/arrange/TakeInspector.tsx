import { useEffect, useMemo, useState } from 'react';
import type { CreativeSession, RecordingTakeRecord } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';
import { useInspectorOperation } from './useInspectorOperation';

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
  const commit = (promise: Promise<CreativeSession>, message: string) =>
    runOperation(promise, message, props.setSession);
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
      runOperation(
        Promise.reject(new Error('This Take has no previewable audio source.')),
        'Take preview started.',
      );
      return;
    }
    if (variant && take.rawAudio && take.processedAudio) {
      if (previewingTake === take.id) {
        runOperation(
          props.api.switchTakeComparisonVariant(selectedVariant),
          `${selectedVariant === 'raw' ? 'Raw' : 'Processed'} comparison selected.`,
          () => {
            setComparisonVariant(selectedVariant);
          },
        );
      } else {
        const comparison = props.api.startTakeComparison(take.id).then(async (status) => {
          if (selectedVariant === 'processed') {
            return await props.api.switchTakeComparisonVariant('processed');
          }
          return status;
        });
        runOperation(comparison, 'Take comparison started.', () => {
          setPreviewingTake(take.id);
          setComparisonVariant(selectedVariant);
        });
      }
    } else {
      runOperation(props.api.previewAsset(assetId, { looped: false }), 'Take preview started.');
    }
  };

  return (
    <section aria-label="Recording takes">
      <header>
        <strong>TAKES</strong>
      </header>
      {context.takes.map((take, index) => {
        const active = context.recordingSession.trackSlots.some(
          (slot) => slot.trackId === take.trackId && slot.activeTakeId === take.id,
        );
        return (
          <div key={take.id}>
            <p>
              Take {index + 1} {active ? '· ACTIVE' : ''}
            </p>
            <button onClick={() => preview(take)}>Preview</button>
            {!active && (
              <button
                onClick={() =>
                  commit(
                    props.api.activateTake(context.recordingSession.id, take.id),
                    'Active Take updated.',
                  )
                }
              >
                Use
              </button>
            )}
            <button
              onClick={() =>
                commit(props.api.placeTakeAsSeparateClip(take.id), 'Take copy placed.')
              }
            >
              Place copy
            </button>
            {take.rawAudio && take.processedAudio && (
              <div role="group" aria-label={`Compare Take ${index + 1}`}>
                <button
                  aria-pressed={previewingTake === take.id && comparisonVariant === 'raw'}
                  onClick={() => preview(take, 'raw')}
                >
                  A · RAW
                </button>
                <button
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
      {operationMessage && <p role="status">{operationMessage}</p>}
    </section>
  );
}
