import { useMemo, useState } from 'react';
import type { AudioStatus, CreativeSession, PluginEntry } from '@/lib/domain';
import {
  MUSICAL_TYPING_MAX_OCTAVE,
  MUSICAL_TYPING_MIN_OCTAVE,
  midiNoteName,
} from '@/lib/musical-typing';
import { useMusicalTyping } from '@/hooks/useMusicalTyping';
import { Icon, Meter } from '../shared/ui';
import { MidiInputPanel, MIDI_INPUT_DEFAULT_OCTAVE } from './MidiInputPanel';

interface WorkspacePlayProps {
  session: CreativeSession;
  audio: AudioStatus;
  missingPluginPaths: string[];
  onOpenPluginEditor: () => void;
  onTogglePluginBypass: (bypassed: boolean) => void;
  onClearPlugin: () => void;
  onCaptureSnapshot: (slot: 'A' | 'B') => void;
  onRecallSnapshot: (slot: 'A' | 'B') => void;
  onSendMidi: (bytes: number[]) => void | Promise<void>;
}

export function WorkspacePlay({
  session,
  audio,
  missingPluginPaths,
  onOpenPluginEditor,
  onTogglePluginBypass,
  onClearPlugin,
  onCaptureSnapshot,
  onRecallSnapshot,
  onSendMidi,
}: WorkspacePlayProps) {
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [octave, setOctave] = useState(MIDI_INPUT_DEFAULT_OCTAVE);
  const inputChannel = audio.inputChannels.find((channel) => channel.index === audio.inputChannel);
  const inputDb = audio.inputPeak > 0 ? 20 * Math.log10(audio.inputPeak) : -90;
  const missingPaths = new Set(missingPluginPaths);
  const loadedPlugins = session.rack.devices
    .filter((device) => device.kind === 'plugin')
    .map(
      (device) =>
        ({
          id: device.id,
          name: device.name,
          vendor: null,
          version: null,
          format: 'VST3',
          path: device.path ?? '',
          bundle: true,
          modifiedAtMs: null,
          scanState: device.path && missingPaths.has(device.path) ? 'quarantined' : 'validated',
        }) as PluginEntry,
    );
  const loadedBypassed =
    session.rack.devices.find((device) => device.kind === 'plugin')?.bypassed ?? false;
  const hasSnapshotA = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:A');
  const hasSnapshotB = session.snapshots.some((snapshot) => snapshot.id === 'snapshot:B');

  const pluginStatus = audio.plugin ?? null;
  const pluginIsInstrument =
    pluginStatus != null && pluginStatus.loaded && pluginStatus.inputChannels === 0;

  const { activeNotes } = useMusicalTyping({
    enabled: pluginIsInstrument,
    octave,
    sendMidi: onSendMidi,
    onOctaveChange: (delta) =>
      setOctave((current) =>
        Math.max(MUSICAL_TYPING_MIN_OCTAVE, Math.min(MUSICAL_TYPING_MAX_OCTAVE, current + delta)),
      ),
  });

  const heldNoteSummary = useMemo(() => {
    if (activeNotes.size === 0) return pluginIsInstrument ? 'No notes held' : null;
    return Array.from(activeNotes)
      .sort((a, b) => a - b)
      .map((note) => midiNoteName(note))
      .join(' ');
  }, [activeNotes, pluginIsInstrument]);

  return (
    <div className="workspace-scroll play-view">
      <section className="play-header">
        <div>
          <span className="eyebrow">LIVE SIGNAL</span>
          <h1>{pluginIsInstrument ? 'MIDI → Tone → Output' : 'Input → Tone → Output'}</h1>
        </div>
        <div className="snapshot-tabs">
          <button className={hasSnapshotA ? 'active' : ''} onClick={() => onRecallSnapshot('A')}>
            A
          </button>
          <button className={hasSnapshotB ? 'active' : ''} onClick={() => onRecallSnapshot('B')}>
            B
          </button>
          <button onClick={() => onCaptureSnapshot(hasSnapshotA ? 'B' : 'A')}>＋</button>
        </div>
      </section>
      <div className="signal-line" />
      <section className="rack-flow">
        {pluginIsInstrument ? (
          <article className="rack-device midi-source-device" aria-label="MIDI source">
            <span className="device-order">IN</span>
            <div className="device-face midi-source-face">
              <span className="meter-label">MIDI</span>
              <i
                className={`midi-led${activeNotes.size > 0 || audio.midiMessages > 0 ? ' active' : ''}`}
                aria-hidden="true"
              />
            </div>
            <h3>
              {audio.midiInputs.length > 0 ? audio.midiInputs.join(' · ') : 'Computer Keyboard'}
            </h3>
            <small>{heldNoteSummary ?? 'Awaiting input'}</small>
          </article>
        ) : (
          <article className="rack-device input-device">
            <span className="device-order">IN</span>
            <div className="device-face live-meter-face">
              <span className="meter-label">INPUT LEVEL</span>
              <Meter value={Math.round(audio.inputPeak * 100)} />
            </div>
            <h3>{inputChannel?.name ?? 'No input channel'}</h3>
            <small>{inputDb.toFixed(1)} dBFS</small>
          </article>
        )}
        {loadedPlugins.map((plugin, index) => (
          <article
            className={[
              'rack-device plugin-device',
              selectedPluginId === plugin.id ? 'selected' : '',
              plugin.scanState === 'quarantined' ? 'missing-dependency' : '',
              pluginIsInstrument ? 'instrument-device' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            key={plugin.id}
            onClick={() => setSelectedPluginId(plugin.id)}
            onDoubleClick={() => {
              if (plugin.scanState === 'validated') onOpenPluginEditor();
            }}
          >
            <span className="device-order">{String(index + 1).padStart(2, '0')}</span>
            <div className={`device-face face-${index}`}>
              <span>{plugin.name.slice(0, 2).toUpperCase()}</span>
              <i />
              {plugin.scanState === 'validated' && (
                <button
                  className="plugin-editor-trigger"
                  aria-label={`Open ${plugin.name} editor`}
                  title="Open plugin editor"
                  onClick={(event) => {
                    event.stopPropagation();
                    setSelectedPluginId(plugin.id);
                    onOpenPluginEditor();
                  }}
                  onDoubleClick={(event) => event.stopPropagation()}
                >
                  <Icon name="sliders" />
                </button>
              )}
            </div>
            <h3>{plugin.name}</h3>
            <small>
              {plugin.scanState === 'quarantined'
                ? 'Missing dependency'
                : pluginIsInstrument
                  ? 'Instrument · Loaded in rack'
                  : 'Loaded in rack'}
            </small>
            <div className="device-controls">
              <button
                onClick={(event) => {
                  event.stopPropagation();
                  onTogglePluginBypass(!loadedBypassed);
                }}
                onDoubleClick={(event) => event.stopPropagation()}
              >
                {loadedBypassed ? 'Enable' : 'Bypass'}
              </button>
              <button
                onClick={(event) => {
                  event.stopPropagation();
                  onClearPlugin();
                }}
                onDoubleClick={(event) => event.stopPropagation()}
              >
                Remove
              </button>
            </div>
          </article>
        ))}
        {loadedPlugins.length === 0 && (
          <article className="rack-device rack-empty">
            <span className="device-order">01</span>
            <div className="device-face">
              <span>—</span>
            </div>
            <h3>No plugin loaded</h3>
            <small>Pick a VST3 from the Library to add it to the rack.</small>
          </article>
        )}
        <article className="rack-device output-device">
          <span className="device-order">OUT</span>
          <div className="device-face">
            <Meter value={Math.round(audio.outputPeak * 100)} />
          </div>
          <h3>Output</h3>
          <small>
            {audio.outputDevice ?? 'No output device'}
            {audio.outputChannels.length > 0
              ? ` · ${audio.outputChannels
                  .slice(0, 2)
                  .map((channel) => channel.name)
                  .join(' + ')}`
              : ''}
          </small>
        </article>
      </section>
      {pluginIsInstrument && (
        <MidiInputPanel
          audio={audio}
          octave={octave}
          onOctaveChange={setOctave}
          activeNotes={activeNotes}
        />
      )}
    </div>
  );
}
