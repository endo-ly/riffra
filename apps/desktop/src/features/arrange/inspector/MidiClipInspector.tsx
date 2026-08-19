import { useEffect, useState } from 'react';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { formatMusicalPosition } from '@/features/arrange/model/arrange-timeline';
import surface from '@/shared/ui/Surface.module.css';
import styles from './ArrangeClipInspector.module.css';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface MidiClipInspectorProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: ArrangeInspectorApi;
}

export function MidiClipInspector(props: MidiClipInspectorProps) {
  const selected = props.session.arrangement.midiClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.length === 1 ? selected[0] : null;
  const [name, setName] = useState(clip?.name ?? '');
  const [startTick, setStartTick] = useState(String(clip?.startTick ?? 0));
  const [durationTicks, setDurationTicks] = useState(String(clip?.durationTicks ?? 1));
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setName(clip?.name ?? '');
    setStartTick(String(clip?.startTick ?? 0));
    setDurationTicks(String(clip?.durationTicks ?? 1));
  }, [clip?.durationTicks, clip?.id, clip?.name, clip?.startTick]);

  const commit = async (operation: Promise<ArrangementMutationResult | null>) => {
    const next = await operation;
    if (next) applyArrangementMutation(next, props.setSession, setMessage);
  };

  if (!clip) {
    return null;
  }

  const patch = (fields: Parameters<ArrangeInspectorApi['updateMidiClip']>[1]) =>
    void commit(props.api.updateMidiClip(clip.id, fields));
  return (
    <div className={styles.inspector}>
      <section className={styles.identity}>
        <span className={styles.art}>♪</span>
        <div>
          <span className={surface.eyebrow}>MIDI CLIP</span>
          <input
            aria-label="MIDI clip name"
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            onBlur={() => {
              const next = name.trim();
              if (next && next !== clip.name) patch({ name: next });
            }}
          />
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
        <button onClick={() => void commit(props.api.duplicateMidiClip(clip.id))}>Duplicate</button>
        <button
          className={styles.danger}
          onClick={() =>
            void commit(props.api.removeTimelineClips([], [clip.id])).then(() =>
              props.setSelectedClipIds([]),
            )
          }
        >
          Delete
        </button>
      </div>
      {message && <p className={styles.message}>{message}</p>}
    </div>
  );
}
