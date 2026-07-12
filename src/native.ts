import { invoke } from "@tauri-apps/api/core";
import type { AudioAnalysis, AudioDeviceProbe, AudioStatus, BootstrapState, LibraryAsset, MidiProbe, ProjectExport, RecordingAsset, RecoveryCandidate, RenderResult, SamplePad, ScanReport, ScratchSession, SeparationResult } from "./domain";
import { defaultSession } from "./domain";

const defaultVst3Root = "C:\\Program Files\\Common Files\\VST3";

export async function bootstrap(): Promise<BootstrapState> {
  try {
    return await invoke<BootstrapState>("get_bootstrap_state");
  } catch {
    return {
      session: defaultSession(),
      recoveredFromGeneration: false,
      safeMode: false,
      recoveryCandidates: [] as RecoveryCandidate[],
      dataRoot: "Browser preview — native persistence is unavailable",
      vst3Root: defaultVst3Root,
    };
  }
}

export async function saveScratch(session: ScratchSession): Promise<void> {
  try {
    await invoke("save_scratch_session", { session });
  } catch {
    localStorage.setItem("riffra.preview.scratch", JSON.stringify(session));
  }
}

export async function restoreRecoveryGeneration(fileName: string): Promise<ScratchSession | null> {
  try {
    return await invoke<ScratchSession>("restore_recovery_generation", { fileName });
  } catch {
    return null;
  }
}

export async function exportScratchSession(): Promise<ProjectExport | null> {
  try {
    return await invoke<ProjectExport>("export_scratch_session");
  } catch {
    return null;
  }
}

export async function importScratchSession(path: string): Promise<ScratchSession | null> {
  try {
    return await invoke<ScratchSession>("import_scratch_session", { path });
  } catch {
    return null;
  }
}

export async function scanVst3Folder(path?: string): Promise<ScanReport> {
  try {
    return await invoke<ScanReport>("scan_vst3_folder", { path: path ?? null });
  } catch {
    return {
      root: path ?? defaultVst3Root,
      startedAtMs: Date.now(),
      finishedAtMs: Date.now(),
      plugins: [],
      issues: [{ path: path ?? defaultVst3Root, message: "Native scanner is unavailable in browser preview." }],
    };
  }

}

export async function listRecordings(query?: string): Promise<RecordingAsset[]> {
  try {
    return await invoke<RecordingAsset[]>("list_recordings", { query: query ?? null });
  } catch {
    return [];
  }
}

export async function searchLibrary(query: string): Promise<LibraryAsset[]> {
  if (!query.trim()) return [];
  try {
    return await invoke<LibraryAsset[]>("search_library", { query });
  } catch {
    return [];
  }
}

export async function updateLibraryAsset(id: string, tag: string | null, note: string | null): Promise<LibraryAsset | null> {
  try {
    return await invoke<LibraryAsset>("update_library_asset", { id, tag, note });
  } catch {
    return null;
  }
}

export async function relatedLibraryAssets(id: string): Promise<LibraryAsset[]> {
  try {
    return await invoke<LibraryAsset[]>("related_library_assets", { id });
  } catch {
    return [];
  }
}

export async function analyzeAudio(path: string): Promise<AudioAnalysis | null> {
  try {
    return await invoke<AudioAnalysis>("analyze_audio", { path });
  } catch {
    return null;
  }
}

export async function probeMidiDevices(): Promise<MidiProbe> {
  try {
    return await invoke<MidiProbe>("probe_midi_devices");
  } catch {
    return {
      inputs: [],
      outputs: [],
      refreshedAtMs: Date.now(),
      message: "MIDI probe is unavailable in browser preview.",
    };
  }
}

export async function probeAudioDevices(): Promise<AudioDeviceProbe> {
  try {
    return await invoke<AudioDeviceProbe>("probe_audio_devices");
  } catch {
    return { drivers: [], midiInputs: [], midiOutputs: [], refreshedAtMs: Date.now(), message: "Audio device probe is unavailable in browser preview." };
  }
}

export async function listSeparations(): Promise<SeparationResult[]> {
  try {
    return await invoke<SeparationResult[]>("list_separations");
  } catch {
    return [];
  }
}

export async function separateChannels(path: string): Promise<SeparationResult | null> {
  try {
    return await invoke<SeparationResult>("separate_channels", { path });
  } catch {
    return null;
  }
}

export async function renderTimeline(): Promise<RenderResult | null> {
  try {
    return await invoke<RenderResult>("render_timeline");
  } catch {
    return null;
  }
}

export async function loadPlugin(path: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("load_plugin", { path });
  } catch {
    return await getAudioStatus();
  }
}

export async function clearPlugin(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("clear_plugin");
  } catch {
    return await getAudioStatus();
  }
}

export async function previewSample(path: string, startMs: number, endMs: number): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("preview_sample", { path, startMs, endMs });
  } catch {
    return await getAudioStatus();
  }
}

export async function stopSamplePreview(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("stop_preview");
  } catch {
    return await getAudioStatus();
  }
}

export async function getAudioStatus(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("get_audio_status");
  } catch {
    return {
      state: "offline",
      driver: null,
      sampleRate: null,
      bufferSize: null,
      roundTripMs: null,
      recording: {
        active: false,
        directory: null,
        sampleRate: null,
        rawChannels: null,
        processedChannels: null,
        samplesWritten: 0,
        droppedBlocks: 0,
      },
      midiInputs: [],
      midiOutputs: [],
      midiInputActive: false,
      midiMessages: 0,
      lastMidiNote: null,
      midiPadMappings: 0,
      midiPadTriggers: 0,
      inputPeak: 0,
      outputPeak: 0,
      invalidSamples: 0,
      message: "Native audio sidecar is not connected.",
    };
  }
}

export async function setEmergencyMute(muted: boolean): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("set_emergency_mute", { muted });
  } catch {
    return {
      state: muted ? "muted" : "offline",
      driver: null,
      sampleRate: null,
      bufferSize: null,
      roundTripMs: null,
      recording: {
        active: false,
        directory: null,
        sampleRate: null,
        rawChannels: null,
        processedChannels: null,
        samplesWritten: 0,
        droppedBlocks: 0,
      },
      midiInputs: [],
      midiOutputs: [],
      midiInputActive: false,
      midiMessages: 0,
      lastMidiNote: null,
      midiPadMappings: 0,
      midiPadTriggers: 0,
      inputPeak: 0,
      outputPeak: 0,
      invalidSamples: 0,
      message: muted ? "Emergency mute is engaged." : "Native audio sidecar is not connected.",
    };
  }
}

export async function startRecording(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("start_recording");
  } catch {
    return await getAudioStatus();
  }
}

export async function stopRecording(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("stop_recording");
  } catch {
    return await getAudioStatus();
  }
}

export async function setPluginBypassed(bypassed: boolean): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("set_plugin_bypassed", { bypassed });
  } catch {
    return await getAudioStatus();
  }
}

export async function recoverAudioDevice(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("recover_audio_device");
  } catch {
    return await getAudioStatus();
  }
}

export async function setAudioDriver(driver: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("set_audio_driver", { driver });
  } catch {
    return await getAudioStatus();
  }
}

export async function openMidiInput(name: string): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("open_midi_input", { name });
  } catch {
    return await getAudioStatus();
  }
}

export async function closeMidiInput(): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("close_midi_input");
  } catch {
    return await getAudioStatus();
  }
}

export async function configureSamplePads(pads: SamplePad[]): Promise<AudioStatus> {
  try {
    return await invoke<AudioStatus>("configure_sample_pads", { pads });
  } catch {
    return await getAudioStatus();
  }
}
