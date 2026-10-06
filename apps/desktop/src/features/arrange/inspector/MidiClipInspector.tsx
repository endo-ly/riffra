import { useEffect, useState } from 'react';
import clsx from 'clsx';
import type { ArrangementMutationResult, CanonicalState, CreativeSession } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { Icon } from '@/shared/ui/primitives';
import { MusicalTimeField } from './MusicalTimeField';
import styles from './Inspector.module.css';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface MidiClipInspectorProps {
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
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
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setName(clip?.name ?? '');
  }, [clip?.id, clip?.name]);

  const commit = async (operation: Promise<ArrangementMutationResult | null>) => {
    const next = await operation;
    if (next) applyArrangementMutation(next, props.applyCanonicalState, setMessage);
  };

  const [quantizeGrid, setQuantizeGrid] = useState(
    String(Math.max(1, Math.round(props.session.arrangement.timebase.ppq / 4))),
  );

  if (!clip) {
    return null;
  }

  const patch = (fields: Parameters<ArrangeInspectorApi['updateMidiClip']>[1]) =>
    void commit(props.api.updateMidiClip(clip.id, fields));
  const noteIds = clip.notes.map((note) => note.id);
  const hasNotes = noteIds.length > 0;
  // Empty ids tell the core to transform every note in the clip.
  const transform = (transpose: number, velocity: number) =>
    void commit(props.api.transformMidiNotes(clip.id, [], transpose, velocity));
  return (
    <div className={styles.inspector}>
      <div className={styles.identity}>
        <span className={styles.identityIcon}>
          <Icon name="note" />
        </span>
        <input
          className={styles.identityName}
          aria-label="MIDI clip name"
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
          onBlur={() => {
            const next = name.trim();
            if (next && next !== clip.name) patch({ name: next });
          }}
        />
      </div>
      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>TIMING</strong>
        </header>
        <div className={styles.fieldPair}>
          <MusicalTimeField
            label="Start"
            kind="position"
            ticks={clip.startTick}
            timebase={props.session.arrangement.timebase}
            onCommit={(startTick) => patch({ startTick })}
          />
          <MusicalTimeField
            label="Length"
            kind="length"
            ticks={clip.durationTicks}
            timebase={props.session.arrangement.timebase}
            onCommit={(durationTicks) => patch({ durationTicks })}
          />
        </div>
      </section>
      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>TRANSFORM</strong>
        </header>
        <div className={styles.transformGrid}>
          <div className={styles.transformRow} aria-label="Transpose">
            <span className={styles.transformLabel}>Pitch</span>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              aria-label="Transpose down octave"
              onClick={() => transform(-12, 0)}
            >
              −12
            </button>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              aria-label="Transpose down 1 semitone"
              onClick={() => transform(-1, 0)}
            >
              −1
            </button>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              aria-label="Transpose up 1 semitone"
              onClick={() => transform(1, 0)}
            >
              +1
            </button>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              aria-label="Transpose up octave"
              onClick={() => transform(12, 0)}
            >
              +12
            </button>
          </div>
          <div className={styles.transformRow} aria-label="Velocity">
            <span className={styles.transformLabel}>Vel</span>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              aria-label="Decrease velocity"
              onClick={() => transform(0, -10)}
            >
              −10
            </button>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              aria-label="Increase velocity"
              onClick={() => transform(0, 10)}
            >
              +10
            </button>
          </div>
          <div className={styles.transformRow}>
            <select
              className={styles.control}
              aria-label="Quantize grid"
              value={quantizeGrid}
              onChange={(event) => setQuantizeGrid(event.target.value)}
              style={{ maxWidth: 88 }}
            >
              <option value={String(props.session.arrangement.timebase.ppq)}>1/4</option>
              <option value={String(Math.round(props.session.arrangement.timebase.ppq / 2))}>
                1/8
              </option>
              <option value={String(Math.round(props.session.arrangement.timebase.ppq / 4))}>
                1/16
              </option>
              <option value={String(Math.round(props.session.arrangement.timebase.ppq / 8))}>
                1/32
              </option>
            </select>
            <button
              type="button"
              className={styles.smallButton}
              disabled={!hasNotes}
              onClick={() =>
                void commit(props.api.quantizeMidiNotes(clip.id, noteIds, Number(quantizeGrid)))
              }
            >
              Quantize
            </button>
          </div>
        </div>
      </section>
      <section className={styles.section}>
        <div className={styles.clipActions}>
          <button
            type="button"
            className={styles.smallButton}
            aria-pressed={clip.muted}
            onClick={() => patch({ muted: !clip.muted })}
          >
            Mute
          </button>
          <button
            type="button"
            className={styles.smallButton}
            aria-pressed={clip.loopEnabled}
            onClick={() => patch({ loopEnabled: !clip.loopEnabled })}
          >
            Loop
          </button>
          <button
            type="button"
            className={styles.smallButton}
            onClick={() => void commit(props.api.duplicateMidiClip(clip.id))}
          >
            Duplicate
          </button>
          <button
            type="button"
            className={clsx(styles.smallButton, styles.danger)}
            onClick={() =>
              void commit(props.api.removeTimelineClips([], [clip.id])).then(() =>
                props.setSelectedClipIds([]),
              )
            }
          >
            Delete
          </button>
        </div>
      </section>
      {message && <p className={styles.message}>{message}</p>}
    </div>
  );
}
