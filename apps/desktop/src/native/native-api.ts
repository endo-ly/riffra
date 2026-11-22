import type { AudioApi } from '@/native/api/audio';
import type { AnalysisApi } from '@/native/api/analysis';
import type { AutomationApi } from '@/native/api/automation';
import type { BootstrapApi } from '@/native/api/bootstrap';
import type { ClipApi } from '@/native/api/clips';
import type { DeviceApi } from '@/native/api/devices';
import type { NativeEventApi } from '@/native/api/events';
import type { HostConnectionApi } from '@/native/api/host-connection';
import type { InstrumentLibraryApi } from '@/native/api/instruments';
import type { JobApi } from '@/native/api/jobs';
import type { LibraryApi } from '@/native/api/library';
import type { MidiNoteApi } from '@/native/api/midi-notes';
import type { MissingDependencyApi } from '@/native/api/missing';
import type { MixerApi } from '@/native/api/mixer';
import type { ProjectApi as NativeProjectApi } from '@/native/api/project';
import type { RecordingApi } from '@/native/api/recording';
import type { RenderApi } from '@/native/api/render';
import type { RigApi } from '@/native/api/rig';
import type { TakeApi } from '@/native/api/takes';
import type { TimelineApi } from '@/native/api/timeline';
import type { TrackApi } from '@/native/api/tracks';
import type { TransportApi } from '@/native/api/transport';
import type { UpdaterApi } from '@/native/api/updater';

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
    RigApi,
    HostConnectionApi,
    UpdaterApi,
    NativeEventApi {}
