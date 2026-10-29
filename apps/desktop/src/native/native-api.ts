import type { AudioApi } from './api/audio';
import type { AnalysisApi } from './api/analysis';
import type { AutomationApi } from './api/automation';
import type { BootstrapApi } from './api/bootstrap';
import type { ClipApi } from './api/clips';
import type { DeviceApi } from './api/devices';
import type { NativeEventApi } from './api/events';
import type { HostConnectionApi } from './api/host-connection';
import type { InstrumentLibraryApi } from './api/instruments';
import type { JobApi } from './api/jobs';
import type { LibraryApi } from './api/library';
import type { MidiNoteApi } from './api/midi-notes';
import type { MissingDependencyApi } from './api/missing';
import type { MixerApi } from './api/mixer';
import type { ProjectApi as NativeProjectApi } from './api/project';
import type { RecordingApi } from './api/recording';
import type { RenderApi } from './api/render';
import type { TakeApi } from './api/takes';
import type { TimelineApi } from './api/timeline';
import type { TrackApi } from './api/tracks';
import type { TransportApi } from './api/transport';
import type { UpdaterApi } from './api/updater';

export interface NativeApi
  extends
    BootstrapApi,
    NativeProjectApi,
    JobApi,
    LibraryApi,
    InstrumentLibraryApi,
    AnalysisApi,
    RenderApi,
    AudioApi,
    RecordingApi,
    ClipApi,
    TrackApi,
    TimelineApi,
    MidiNoteApi,
    TakeApi,
    AutomationApi,
    MixerApi,
    DeviceApi,
    TransportApi,
    MissingDependencyApi,
    HostConnectionApi,
    UpdaterApi,
    NativeEventApi {}
