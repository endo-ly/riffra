import { useEffect, useState } from 'react';
import clsx from 'clsx';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { formatMusicalPosition } from '@/features/arrange/model/arrange-timeline';
import { Icon } from '@/shared/ui/primitives';
import styles from './Inspector.module.css';
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
        <span className={styles.identityMeta}>
          {formatMusicalPosition(clip.startTick, props.session.arrangement.timebase)}
        </span>
      </div>
      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>TIMING</strong>
        </header>
        <div className={styles.fieldPair}>
          <label className={styles.field}>
            <span>Start</span>
            <input
              className={clsx(styles.control, styles.mono)}
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
          <label className={styles.field}>
            <span>Length</span>
            <input
              className={clsx(styles.control, styles.mono)}
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
        </div>
      </section>
      <section className={styles.section}>
        <div className={styles.clipActions}>
          <div className={styles.segmented} role="group" aria-label="Clip state">
            <button
              type="button"
              aria-pressed={clip.muted}
              onClick={() => patch({ muted: !clip.muted })}
            >
              Mute
            </button>
            <button
              type="button"
              aria-pressed={clip.loopEnabled}
              onClick={() => patch({ loopEnabled: !clip.loopEnabled })}
            >
              Loop
            </button>
          </div>
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
