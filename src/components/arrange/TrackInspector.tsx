import { useCallback, useEffect, useState } from 'react';
import type { AudioStatus, CreativeSession, Track } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { TrackPluginChainEditor } from './TrackPluginChainEditor';

interface TrackInspectorProps {
  track: Track;
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  audio: AudioStatus;
  api: NativeApi;
}

export function TrackInspector(props: TrackInspectorProps) {
  const [name, setName] = useState(props.track.name);
  const [gainDb, setGainDb] = useState(props.track.gainDb);
  const [pan, setPan] = useState(props.track.pan);
  useEffect(() => setName(props.track.name), [props.track.id, props.track.name]);
  useEffect(() => setGainDb(props.track.gainDb), [props.track.id, props.track.gainDb]);
  useEffect(() => setPan(props.track.pan), [props.track.id, props.track.pan]);
  const commit = useCallback(
    (operation: Promise<CreativeSession>, _message: string) => {
      void operation.then(props.setSession);
    },
    [props.setSession],
  );
  const setInstrument = () => {
    const path = window.prompt('VST3 instrument path', props.track.instrument?.path ?? '')?.trim();
    if (path) commit(props.api.setTrackInstrument(props.track.id, path), 'Instrument updated.');
  };
  return (
    <>
      <section>
        <header>
          <strong>TRACK</strong>
        </header>
        <label>
          Name
          <input
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            onBlur={() => {
              const next = name.trim();
              if (next && next !== props.track.name) {
                commit(props.api.updateTrack(props.track.id, { name: next }), 'Track renamed.');
              } else {
                setName(props.track.name);
              }
            }}
          />
        </label>
      </section>
      {props.track.kind === 'audio' ? (
        <section>
          <header>
            <strong>INPUT</strong>
          </header>
          <select
            aria-label="Audio input"
            value={props.track.audioInput?.channelIndex ?? ''}
            onChange={(event) =>
              commit(
                props.api.setTrackAudioInput(
                  props.track.id,
                  event.currentTarget.value === '' ? null : Number(event.currentTarget.value),
                ),
                'Audio input updated.',
              )
            }
          >
            <option value="">None</option>
            {props.audio.inputChannels.map((channel) => (
              <option key={channel.index} value={channel.index}>
                {channel.name}
              </option>
            ))}
            {props.track.audioInput &&
              !props.audio.inputChannels.some(
                (channel) => channel.index === props.track.audioInput?.channelIndex,
              ) && (
                <option value={props.track.audioInput.channelIndex}>
                  Input {props.track.audioInput.channelIndex + 1} · Unavailable
                </option>
              )}
          </select>
        </section>
      ) : (
        <>
          <section>
            <header>
              <strong>MIDI INPUT</strong>
            </header>
            <select
              aria-label="MIDI input"
              value={props.track.midiInput.deviceId ?? ''}
              onChange={(event) =>
                commit(
                  props.api.setTrackMidiInput(props.track.id, {
                    ...props.track.midiInput,
                    deviceId: event.currentTarget.value || undefined,
                  }),
                  'MIDI input updated.',
                )
              }
            >
              <option value="">All Inputs</option>
              {props.audio.midiInputs.map((device) => (
                <option key={device.id} value={device.id}>
                  {device.name}
                </option>
              ))}
            </select>
            <select
              aria-label="MIDI channel"
              value={props.track.midiInput.channel ?? ''}
              onChange={(event) =>
                commit(
                  props.api.setTrackMidiInput(props.track.id, {
                    ...props.track.midiInput,
                    channel: event.currentTarget.value
                      ? Number(event.currentTarget.value)
                      : undefined,
                  }),
                  'MIDI channel updated.',
                )
              }
            >
              <option value="">All Channels</option>
              {Array.from({ length: 16 }, (_, index) => index + 1).map((channel) => (
                <option key={channel} value={channel}>
                  Channel {channel}
                </option>
              ))}
            </select>
          </section>
          <section>
            <header>
              <strong>INSTRUMENT</strong>
            </header>
            <p>{props.track.instrument?.name ?? 'None'}</p>
            <button onClick={setInstrument}>
              {props.track.instrument ? 'Change' : 'Choose Instrument'}
            </button>
            {props.track.instrument && (
              <>
                <button
                  onClick={() =>
                    void props.api.openTrackPluginEditor(props.track.id, props.track.instrument!.id)
                  }
                >
                  Edit
                </button>
                <button
                  onClick={() =>
                    commit(props.api.clearTrackInstrument(props.track.id), 'Instrument removed.')
                  }
                >
                  Clear
                </button>
              </>
            )}
          </section>
        </>
      )}
      <section>
        <header>
          <strong>MONITORING</strong>
        </header>
        {(['off', 'auto', 'on'] as const).map((monitoring) => (
          <button
            key={monitoring}
            aria-pressed={props.track.monitoring === monitoring}
            onClick={() =>
              commit(props.api.updateTrack(props.track.id, { monitoring }), 'Monitoring updated.')
            }
          >
            {monitoring === 'off' ? 'Off' : monitoring === 'auto' ? 'Auto' : 'On'}
          </button>
        ))}
      </section>
      <TrackPluginChainEditor track={props.track} api={props.api} commit={commit} />
      <section>
        <header>
          <strong>MIX</strong>
        </header>
        <label>
          Volume
          <input
            type="range"
            min="-60"
            max="12"
            step="0.5"
            value={gainDb}
            onChange={(event) => setGainDb(Number(event.currentTarget.value))}
            onPointerUp={() => {
              if (gainDb !== props.track.gainDb)
                commit(props.api.updateTrack(props.track.id, { gainDb }), 'Volume updated.');
            }}
            onKeyUp={() => {
              if (gainDb !== props.track.gainDb)
                commit(props.api.updateTrack(props.track.id, { gainDb }), 'Volume updated.');
            }}
          />
        </label>
        <label>
          Pan
          <input
            type="range"
            min="-1"
            max="1"
            step="0.05"
            value={pan}
            onChange={(event) => setPan(Number(event.currentTarget.value))}
            onPointerUp={() => {
              if (pan !== props.track.pan)
                commit(props.api.updateTrack(props.track.id, { pan }), 'Pan updated.');
            }}
            onKeyUp={() => {
              if (pan !== props.track.pan)
                commit(props.api.updateTrack(props.track.id, { pan }), 'Pan updated.');
            }}
          />
        </label>
      </section>
    </>
  );
}
