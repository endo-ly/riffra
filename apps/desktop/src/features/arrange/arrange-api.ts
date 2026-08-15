import type {
  ArrangeApi,
  AudioApi,
  AnalysisApi,
  JobApi,
  NativeEventApi,
  ProjectApi,
  TransportApi,
} from '@/native/native-api';

/** Native capabilities required by the Arrange workspace shell. */
export type ArrangeWorkspaceApi = ArrangeApi &
  Pick<AudioApi, 'getAudioStatus' | 'sendMidiToTrack' | 'panicMidiTrack' | 'previewAsset'> &
  Pick<AnalysisApi, 'analyzeAsset'> &
  Pick<JobApi, 'scanVst3Folder'> &
  Pick<NativeEventApi, 'onTransportStatus'> &
  Pick<ProjectApi, 'importMidiBytes'> &
  Pick<TransportApi, 'seekTimeline' | 'retryRuntimeProjection'>;

/** Native capabilities required by Arrange inspectors. */
export type ArrangeInspectorApi = ArrangeApi &
  Pick<AudioApi, 'previewAsset'> &
  Pick<JobApi, 'scanVst3Folder'>;
