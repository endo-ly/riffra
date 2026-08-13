import { useCallback, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type {
  AssetId,
  AudioAnalysis,
  AudioStatus,
  BackgroundJobStatus,
  CreativeSession,
  DesignTool,
  LibraryAsset,
  RecordingAsset,
  SeparationResult,
} from '@/model/domain';
import type { ArrangeApi, AudioApi, DesignApi, JobApi, RecordingApi } from '@/native/native-api';
import { isUsableRecording } from '@/shared/recordings';
import { toAssetId } from '@/native/contracts';
import { useSampleKeyboard } from '@/features/design/sample/useSampleKeyboard';

type DesignFeatureApi = Pick<
  AudioApi,
  'previewAsset' | 'stopSamplePreview' | 'stopSamplePreviewKey'
> &
  Pick<ArrangeApi, 'addAudioClipToArrangement'> &
  Pick<DesignApi, 'analyzeAsset' | 'listSeparations'> &
  Pick<JobApi, 'startAnalysisJob' | 'startSeparationJob'> &
  Pick<RecordingApi, 'createSamplePad' | 'updateSamplePad' | 'removeSamplePad'>;

type BackgroundJobRunner = <J extends BackgroundJobStatus>(
  start: () => Promise<J>,
  onCompleted: (result: NonNullable<J['result']>) => void,
  onFailed: (message: string) => void,
) => Promise<boolean>;

type SessionOperationRunner = <T>(
  operation: () => Promise<T | null>,
  label: string,
) => Promise<T | null>;

interface UseDesignOptions {
  api: DesignFeatureApi;
  recordings: RecordingAsset[];
  session: CreativeSession | null;
  targetAssetId?: AssetId;
  setAudio: Dispatch<SetStateAction<AudioStatus>>;
  setSession: (session: CreativeSession) => void;
  openAssetInDesign: (assetId: AssetId, tool: DesignTool) => Promise<void>;
  runBackgroundJob: BackgroundJobRunner;
  runSessionOp: SessionOperationRunner;
}

/** Owns Design analysis, reference comparison, separation, and SamplePad state. */
export function useDesign({
  api,
  recordings,
  session,
  targetAssetId,
  setAudio,
  setSession,
  openAssetInDesign,
  runBackgroundJob,
  runSessionOp,
}: UseDesignOptions) {
  const {
    analyzeAsset,
    addAudioClipToArrangement,
    createSamplePad: createSamplePadApi,
    previewAsset: previewAssetApi,
    removeSamplePad: removeSamplePadApi,
    startAnalysisJob,
    startSeparationJob,
    stopSamplePreview,
    stopSamplePreviewKey,
    listSeparations,
    updateSamplePad: updateSamplePadApi,
  } = api;
  const [separations, setSeparations] = useState<SeparationResult[]>([]);
  const [separationBusy, setSeparationBusy] = useState<string | null>(null);
  const [separationMessage, setSeparationMessage] = useState(
    'Ready for a local stereo channel split.',
  );
  const [separationPreviewingAssetId, setSeparationPreviewingAssetId] = useState<AssetId | null>(
    null,
  );
  const [previewPadId, setPreviewPadId] = useState<string | null>(null);
  const [analysis, setAnalysis] = useState<AudioAnalysis | null>(null);
  const [referenceId, setReferenceId] = useState<string | null>(null);
  const [referencePreviewingId, setReferencePreviewingId] = useState<string | null>(null);
  const [referenceSyncPreviewing, setReferenceSyncPreviewing] = useState(false);
  const [referenceLoopPreview, setReferenceLoopPreview] = useState(false);
  const [referenceAnalyses, setReferenceAnalyses] = useState<Record<string, AudioAnalysis>>({});

  const reloadSeparations = useCallback(async () => {
    setSeparations(await listSeparations());
  }, [listSeparations]);

  const openRecordingAnalysis = useCallback(
    async (recording: RecordingAsset) => {
      if (!isUsableRecording(recording) || recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await runBackgroundJob(
        () => startAnalysisJob(assetId),
        (result) => {
          setAnalysis(result);
          void openAssetInDesign(assetId, 'analyze');
        },
        () => setAnalysis(null),
      );
    },
    [openAssetInDesign, runBackgroundJob, startAnalysisJob],
  );

  const openLibraryAssetAnalysis = useCallback(
    async (asset: LibraryAsset) => {
      if (asset.kind !== 'audio') return;
      const assetId = toAssetId(asset.id);
      const result = await analyzeAsset(assetId);
      if (!result) return;
      setAnalysis(result);
      await openAssetInDesign(assetId, 'analyze');
    },
    [analyzeAsset, openAssetInDesign],
  );

  const selectReference = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      setReferenceId(recording.id);
      if (referenceAnalyses[recording.id]) return;
      const next = await analyzeAsset(assetId);
      if (next) setReferenceAnalyses((current) => ({ ...current, [recording.id]: next }));
    },
    [analyzeAsset, referenceAnalyses],
  );

  const previewReference = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await stopSamplePreview();
      setAudio(await previewAssetApi(assetId, { looped: referenceLoopPreview }));
      setReferencePreviewingId(recording.id);
      setReferenceSyncPreviewing(false);
    },
    [previewAssetApi, referenceLoopPreview, setAudio, stopSamplePreview],
  );

  const previewReferencePair = useCallback(async () => {
    if (!analysis || !targetAssetId || !referenceId) return;
    const reference = recordings.find((recording) => recording.id === referenceId);
    if (!reference) return;
    const referenceAssetId = reference.processedAssetId ?? reference.rawAssetId;
    if (!referenceAssetId) return;
    await stopSamplePreview();
    await previewAssetApi(targetAssetId, { looped: referenceLoopPreview });
    setAudio(await previewAssetApi(referenceAssetId, { looped: referenceLoopPreview }));
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(true);
  }, [
    analysis,
    previewAssetApi,
    recordings,
    referenceId,
    referenceLoopPreview,
    setAudio,
    stopSamplePreview,
    targetAssetId,
  ]);

  const stopReferencePreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setReferencePreviewingId(null);
    setReferenceSyncPreviewing(false);
  }, [setAudio, stopSamplePreview]);

  const runSeparation = useCallback(
    async (recording: RecordingAsset) => {
      if (recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      await openAssetInDesign(assetId, 'separate');
      setSeparationBusy(recording.id);
      setSeparationMessage('Writing Left / Right WAV assets…');
      await runBackgroundJob(
        () => startSeparationJob(assetId),
        (result) => {
          setSeparations((current) => [result, ...current.filter((item) => item.id !== result.id)]);
          setSeparationMessage(result.message);
        },
        (message) => setSeparationMessage(`Separation failed: ${message}`),
      );
      setSeparationBusy(null);
    },
    [openAssetInDesign, runBackgroundJob, startSeparationJob],
  );

  const previewSeparation = useCallback(
    async (assetId: AssetId) => {
      setAudio(await previewAssetApi(assetId, {}));
      setSeparationPreviewingAssetId(assetId);
    },
    [previewAssetApi, setAudio],
  );

  const stopSeparationPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setSeparationPreviewingAssetId(null);
  }, [setAudio, stopSamplePreview]);

  const addSeparationToTimeline = useCallback(
    async (assetId: AssetId, name: string, _durationMs: number) => {
      if (!session) return;
      const next = await runSessionOp(
        () => addAudioClipToArrangement(assetId, name),
        'Add clip to timeline',
      );
      if (next) setSession(next);
    },
    [addAudioClipToArrangement, runSessionOp, session, setSession],
  );

  const previewSamplePad = useCallback(
    async (pad: CreativeSession['playState']['sampleInstrument']['pads'][number]) => {
      const nextAudio = await previewAssetApi(pad.assetId, {
        startMs: pad.startMs,
        endMs: pad.endMs,
        looped: pad.loopEnabled,
        gain: Math.pow(10, (pad.gainDb ?? 0) / 20),
        voiceKey: pad.midiKey,
      });
      setAudio(nextAudio);
      setPreviewPadId(pad.id);
    },
    [previewAssetApi, setAudio],
  );

  useSampleKeyboard({ session, previewSamplePad, stopSamplePreviewKey, setAudio });

  const stopPreview = useCallback(async () => {
    setAudio(await stopSamplePreview());
    setPreviewPadId(null);
  }, [setAudio, stopSamplePreview]);

  const createSamplePad = useCallback(
    async (recording: RecordingAsset) => {
      if (!session || recording.error) return;
      const assetId = recording.processedAssetId ?? recording.rawAssetId;
      if (!assetId) return;
      const { session: nextSession, audio: nextAudio } = await createSamplePadApi(
        assetId,
        recording.name,
      );
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [createSamplePadApi, session, setAudio, setSession],
  );

  const updateSamplePad = useCallback(
    async (
      padId: string,
      patch: {
        startMs?: number;
        endMs?: number;
        gainDb?: number;
        loopEnabled?: boolean;
      },
    ) => {
      const { session: nextSession, audio: nextAudio } = await updateSamplePadApi(padId, patch);
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [setAudio, setSession, updateSamplePadApi],
  );

  const removeSamplePad = useCallback(
    async (padId: string) => {
      const { session: nextSession, audio: nextAudio } = await removeSamplePadApi(padId);
      setSession(nextSession);
      setAudio(nextAudio);
    },
    [removeSamplePadApi, setAudio, setSession],
  );

  return {
    separations,
    separationBusy,
    separationMessage,
    separationPreviewingAssetId,
    previewPadId,
    setPreviewPadId,
    reloadSeparations,
    analysis,
    referenceId,
    referencePreviewingId,
    referenceSyncPreviewing,
    referenceLoopPreview,
    setReferenceLoopPreview,
    referenceAnalyses,
    openRecordingAnalysis,
    openLibraryAssetAnalysis,
    selectReference,
    previewReference,
    previewReferencePair,
    stopReferencePreview,
    runSeparation,
    previewSeparation,
    stopSeparationPreview,
    addSeparationToTimeline,
    previewSamplePad,
    stopPreview,
    createSamplePad,
    updateSamplePad,
    removeSamplePad,
  };
}
