import type { AudioApi } from '@/native/native-api';

/** Native capabilities required by the global audio monitor. */
export type AudioMonitorApi = Pick<AudioApi, 'previewMasterGainDb' | 'setMasterGainDb'>;
