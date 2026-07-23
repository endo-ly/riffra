import { useEffect, useMemo, useState } from 'react';
import type { CreativeSession, RecordingTakeRecord } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import type { ArrangeSelection } from '@/hooks/arrange/useArrangeEditor';

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

  useEffect(
    () => () => {
      void props.api.stopTakeComparison();
    },
    [props.api],
  );

  if (!context || context.takes.length === 0) return null;
  const commit = (promise: Promise<CreativeSession>) => void promise.then(props.setSession);
  const preview = (take: RecordingTakeRecord, variant?: 'raw' | 'processed') => {
    const selectedVariant =
      variant ??
      arrangement.audioClips.find((clip) => clip.recordingTakeId === take.id)?.takeVariant ??
      'processed';
    const assetId =
      selectedVariant === 'raw'
        ? (take.rawAudioAssetId ?? take.processedAudioAssetId)
        : (take.processedAudioAssetId ?? take.rawAudioAssetId);
    if (!assetId) return;
    if (variant && take.rawAudioAssetId && take.processedAudioAssetId) {
      if (previewingTake === take.id) {
        void props.api.switchTakeComparisonVariant(selectedVariant);
      } else {
        void props.api.startTakeComparison(take.id).then(() => {
          if (selectedVariant === 'processed')
            return props.api.switchTakeComparisonVariant('processed');
          return undefined;
        });
      }
      setPreviewingTake(take.id);
      setComparisonVariant(selectedVariant);
    } else {
      void props.api.previewAsset(assetId, { looped: false });
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
                onClick={() => commit(props.api.activateTake(context.recordingSession.id, take.id))}
              >
                Use
              </button>
            )}
            <button onClick={() => commit(props.api.placeTakeAsSeparateClip(take.id))}>
              Place copy
            </button>
            {take.rawAudioAssetId && take.processedAudioAssetId && (
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
    </section>
  );
}
