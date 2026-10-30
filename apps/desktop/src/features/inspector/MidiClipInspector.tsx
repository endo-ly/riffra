import type { ArrangementMutationResult, CreativeSession } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { Icon } from '@/shared/ui/primitives';
import { MenuButton } from '@/shared/ui/MenuButton';
import { useEffect, useState, type CSSProperties } from 'react';
import type { MidiClipPatch } from '@/model/generated';
import { resolveTrackColorById } from '@/model/track-color';
import { ClipToggles } from './ClipToggles';
import styles from './Inspector.module.css';
import { MusicalTimeField } from './MusicalTimeField';

interface MidiClipInspectorProps {
  session: CreativeSession;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
}

export function MidiClipInspector(props: MidiClipInspectorProps) {
  const { api, commit: sessionCommit } = useSessionContext();
  const selected = props.session.arrangement.midiClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.length === 1 ? selected[0] : null;
  const [name, setName] = useState(clip?.name ?? '');
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setName(clip?.name ?? '');
  }, [clip?.id, clip?.name]);

  const commit = (operation: Promise<ArrangementMutationResult>) => {
    setMessage(null);
    return sessionCommit(operation, { onError: setMessage });
  };

  const [quantizeGrid, setQuantizeGrid] = useState(
    String(Math.max(1, Math.round(props.session.arrangement.timebase.ppq / 4))),
  );

  if (!clip) {
    return null;
  }

  const patch = (fields: MidiClipPatch) => void commit(api.updateMidiClip(clip.id, fields));
  const noteIds = clip.notes.map((note) => note.id);
  const hasNotes = noteIds.length > 0;
  // Empty ids tell the core to transform every note in the clip.
  const transform = (transpose: number, velocity: number) =>
    void commit(api.transformMidiNotes(clip.id, [], transpose, velocity));
  const tracks = props.session.arrangement.tracks;
  const trackName = tracks.find((track) => track.id === clip.trackId)?.name;
  return (
    <div
      className={styles.inspector}
      style={{ '--track-color': resolveTrackColorById(tracks, clip.trackId) } as CSSProperties}
    >
      <div className={styles.identity}>
        <span className={styles.identityTile}>
          <Icon name="note" />
        </span>
        <div className={styles.identityBlock}>
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
          <small>MIDI clip{trackName && ` · ${trackName}`}</small>
        </div>
        <div className={styles.identityTools}>
          <ClipToggles
            muted={clip.muted}
            loopEnabled={clip.loopEnabled}
            onToggleMuted={() => patch({ muted: !clip.muted })}
            onToggleLoop={() => patch({ loopEnabled: !clip.loopEnabled })}
          />
          <MenuButton
            label="Clip actions"
            items={[
              {
                label: 'Duplicate',
                onClick: () => void commit(api.duplicateMidiClip(clip.id)),
              },
              { separator: true },
              {
                label: 'Delete',
                danger: true,
                onClick: () =>
                  void commit(api.removeTimelineClips([], [clip.id])).then((result) => {
                    if (result) props.setSelectedClipIds([]);
                  }),
              },
            ]}
          />
        </div>
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
        <div className={styles.rows}>
          <div className={styles.labeledRow}>
            <span className={styles.rowLabel}>Pitch</span>
            <div className={styles.segmented} role="group" aria-label="Transpose">
              <button
                type="button"
                disabled={!hasNotes}
                aria-label="Transpose down octave"
                onClick={() => transform(-12, 0)}
              >
                −12
              </button>
              <button
                type="button"
                disabled={!hasNotes}
                aria-label="Transpose down 1 semitone"
                onClick={() => transform(-1, 0)}
              >
                −1
              </button>
              <button
                type="button"
                disabled={!hasNotes}
                aria-label="Transpose up 1 semitone"
                onClick={() => transform(1, 0)}
              >
                +1
              </button>
              <button
                type="button"
                disabled={!hasNotes}
                aria-label="Transpose up octave"
                onClick={() => transform(12, 0)}
              >
                +12
              </button>
            </div>
          </div>
          <div className={styles.labeledRow}>
            <span className={styles.rowLabel}>Velocity</span>
            <div className={styles.segmented} role="group" aria-label="Velocity">
              <button
                type="button"
                disabled={!hasNotes}
                aria-label="Decrease velocity"
                onClick={() => transform(0, -10)}
              >
                −10
              </button>
              <button
                type="button"
                disabled={!hasNotes}
                aria-label="Increase velocity"
                onClick={() => transform(0, 10)}
              >
                +10
              </button>
            </div>
          </div>
          <div className={styles.labeledRow}>
            <span className={styles.rowLabel}>Quantize</span>
            <div className={styles.splitControl}>
              <select
                aria-label="Quantize grid"
                value={quantizeGrid}
                onChange={(event) => setQuantizeGrid(event.target.value)}
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
                disabled={!hasNotes}
                onClick={() =>
                  void commit(api.quantizeMidiNotes(clip.id, noteIds, Number(quantizeGrid)))
                }
              >
                Apply
              </button>
            </div>
          </div>
        </div>
      </section>
      {message && <p className={styles.message}>{message}</p>}
    </div>
  );
}
