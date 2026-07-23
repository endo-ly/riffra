import { useEffect, useState } from 'react';
import type { CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { formatMusicalPosition } from '@/lib/arrange-timeline';
import styles from './ArrangeClipInspector.module.css';

interface MidiClipInspectorProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: NativeApi;
}

export function MidiClipInspector(props: MidiClipInspectorProps) {
  const selected = props.session.arrangement.midiClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.at(-1) ?? null;
  const [name, setName] = useState(clip?.name ?? '');
  const [startTick, setStartTick] = useState(String(clip?.startTick ?? 0));
  const [durationTicks, setDurationTicks] = useState(String(clip?.durationTicks ?? 1));

  useEffect(() => {
    setName(clip?.name ?? '');
    setStartTick(String(clip?.startTick ?? 0));
    setDurationTicks(String(clip?.durationTicks ?? 1));
  }, [clip?.durationTicks, clip?.id, clip?.name, clip?.startTick]);

  const commit = async (operation: Promise<CreativeSession | null>) => {
    const next = await operation;
    if (next) props.setSession(next);
  };

  if (!clip) {
    return (
      <div className={styles.empty}>
        <span className={styles.emptyGlyph}>♪</span>
        <strong>No MIDI Clip selected</strong>
        <p>Select a MIDI Clip to edit its timing, loop and mute state.</p>
      </div>
    );
  }

  const patch = (fields: Parameters<NativeApi['updateMidiClip']>[1]) =>
    void commit(props.api.updateMidiClip(clip.id, fields));
  const duplicate = () => void commit(props.api.duplicateMidiClip(clip.id));

  return (
    <div className={styles.inspector}>
      <section className={styles.identity}>
        <span className={styles.art}>♪</span>
        <div>
          <span className="eyebrow">
            {selected.length > 1 ? `${selected.length} MIDI CLIPS` : 'MIDI CLIP'}
          </span>
          <input
            aria-label="MIDI clip name"
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            onBlur={() => {
              const next = name.trim();
              if (next && next !== clip.name) patch({ name: next });
            }}
          />
          <small>
            {clip.notes.length} notes · {clip.events.length} controller events
          </small>
        </div>
      </section>
      <section className={styles.section}>
        <header>
          <strong>Timing</strong>
          <span>{formatMusicalPosition(clip.startTick, props.session.arrangement.timebase)}</span>
        </header>
        <label>
          <span>Start tick</span>
          <input
            type="number"
            min="0"
            value={startTick}
            onChange={(event) => setStartTick(event.currentTarget.value)}
            onBlur={() => {
              const value = Number(startTick);
              if (Number.isFinite(value) && value >= 0 && value !== clip.startTick)
                patch({ startTick: value });
            }}
          />
        </label>
        <label>
          <span>Duration ticks</span>
          <input
            type="number"
            min="1"
            value={durationTicks}
            onChange={(event) => setDurationTicks(event.currentTarget.value)}
            onBlur={() => {
              const value = Number(durationTicks);
              if (Number.isFinite(value) && value > 0 && value !== clip.durationTicks)
                patch({ durationTicks: value });
            }}
          />
        </label>
      </section>
      <div className={styles.toggles}>
        <button
          className={clip.muted ? styles.active : ''}
          onClick={() => patch({ muted: !clip.muted })}
        >
          Mute
        </button>
        <button
          className={clip.loopEnabled ? styles.active : ''}
          onClick={() => patch({ loopEnabled: !clip.loopEnabled })}
        >
          Loop
        </button>
      </div>
      <div className={styles.actions}>
        <button onClick={duplicate}>
          Duplicate{selected.length > 1 ? ` (${selected.length})` : ''}
        </button>
        <button
          className={styles.danger}
          onClick={() =>
            void commit(
              props.api.removeTimelineClips(
                [],
                selected.map((item) => item.id),
              ),
            ).then(() => props.setSelectedClipIds([]))
          }
        >
          Delete{selected.length > 1 ? ` (${selected.length})` : ''}
        </button>
      </div>
    </div>
  );
}
