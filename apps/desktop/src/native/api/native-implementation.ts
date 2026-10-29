import type { NativeApi } from '../native-api';
import { analysisApi } from './analysis';
import { audioApi } from './audio';
import { automationApi } from './automation';
import { bootstrapApi } from './bootstrap';
import { clipsApi } from './clips';
import { deviceApi } from './devices';
import { eventApi } from './events';
import { hostConnectionApi } from './host-connection';
import { instrumentLibraryApi } from './instruments';
import { jobsApi } from './jobs';
import { libraryApi } from './library';
import { midiNoteApi } from './midi-notes';
import { missingDependencyApi } from './missing';
import { mixerApi } from './mixer';
import { projectApi } from './project';
import { recordingApi } from './recording';
import { renderApi } from './render';
import { takesApi } from './takes';
import { timelineApi } from './timeline';
import { tracksApi } from './tracks';
import { transportApi } from './transport';
import { updaterApi } from './updater';

export function createNativeApi(): NativeApi {
  return {
    ...clipsApi,
    ...tracksApi,
    ...timelineApi,
    ...midiNoteApi,
    ...takesApi,
    ...automationApi,
    ...mixerApi,
    ...deviceApi,
    ...libraryApi,
    ...instrumentLibraryApi,
    ...projectApi,
    ...audioApi,
    ...bootstrapApi,
    ...analysisApi,
    ...jobsApi,
    ...missingDependencyApi,
    ...recordingApi,
    ...renderApi,
    ...transportApi,
    ...updaterApi,
    ...eventApi,
    ...hostConnectionApi,
  };
}
