import type { NativeApi } from '../native-api';
import * as arrangeApi from './arrange';
import * as audioApi from './audio';
import * as bootstrapApi from './bootstrap';
import * as designApi from './design';
import { eventApi } from './events';
import * as jobsApi from './jobs';
import * as libraryApi from './library';
import * as missingApi from './missing';
import * as presentationApi from './presentation';
import * as projectApi from './project';
import * as rackApi from './rack';
import * as recordingApi from './recording';
import * as renderApi from './render';
import * as samplePadApi from './sample-pad';
import * as transportApi from './transport';

export function createNativeApi(): NativeApi {
  return {
    ...bootstrapApi,
    ...projectApi,
    ...jobsApi,
    ...libraryApi,
    ...designApi,
    ...renderApi,
    ...audioApi,
    ...recordingApi,
    ...samplePadApi,
    ...arrangeApi,
    ...rackApi,
    ...transportApi,
    ...presentationApi,
    ...missingApi,
    ...eventApi,
  };
}

export const defaultNativeApi: NativeApi = createNativeApi();
