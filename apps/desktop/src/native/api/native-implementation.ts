import type { NativeApi } from '@/native/native-api';
import { analysisApi } from '@/native/api/analysis';
import { audioApi } from '@/native/api/audio';
import { automationApi } from '@/native/api/automation';
import { bootstrapApi } from '@/native/api/bootstrap';
import { clipsApi } from '@/native/api/clips';
import { deviceApi } from '@/native/api/devices';
import { eventApi } from '@/native/api/events';
import { hostConnectionApi } from '@/native/api/host-connection';
import { instrumentLibraryApi } from '@/native/api/instruments';
import { jobsApi } from '@/native/api/jobs';
import { libraryApi } from '@/native/api/library';
import { midiNoteApi } from '@/native/api/midi-notes';
import { missingDependencyApi } from '@/native/api/missing';
import { mixerApi } from '@/native/api/mixer';
import { projectApi } from '@/native/api/project';
import { recordingApi } from '@/native/api/recording';
import { renderApi } from '@/native/api/render';
import { takesApi } from '@/native/api/takes';
import { timelineApi } from '@/native/api/timeline';
import { tracksApi } from '@/native/api/tracks';
import { transportApi } from '@/native/api/transport';
import { updaterApi } from '@/native/api/updater';

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

export const nativeApiMethodNames = Object.keys(createNativeApi()) as (keyof NativeApi)[];
