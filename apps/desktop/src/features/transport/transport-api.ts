import type { ArrangeApi, ProjectSettingsApi } from '@/native/native-api';

/** Native capabilities required by the transport controls. */
export type TransportControlsApi = Pick<
  ArrangeApi,
  'updateArrangementTimebase' | 'updateTimelineLoopRange'
> &
  Pick<ProjectSettingsApi, 'updateSessionSettings'>;
