import { invoke } from "@tauri-apps/api/core";
import type { AudioAnalysis, AudioStatus, BootstrapState, MidiProbe, RecordingAsset, ScanReport, ScratchSession, SeparationResult } from "./domain";
import { defaultSession } from "./domain";

const defaultVst3Root = "C:\\Program Files\\Common Files\\VST3";

export async function bootstrap(): Promise<BootstrapState> {
  try {
    return await invoke<BootstrapState>("get_bootstrap_state");
  } catch {
    return {
      session: defaultSession(),
      recoveredFromGeneration: false,
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
