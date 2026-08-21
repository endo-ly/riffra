import { useMemo, useState } from 'react';
import clsx from 'clsx';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { timelineObjectEndTick } from '@/features/arrange/model/arrange-timeline';
import { Icon } from '@/shared/ui/primitives';
import styles from './Inspector.module.css';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface MultiClipInspectorProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedAudioClipIds: string[];
  selectedMidiClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: ArrangeInspectorApi;
}

export function MultiClipInspector(props: MultiClipInspectorProps) {
  const [message, setMessage] = useState<string | null>(null);
  const audioClips = useMemo(
    () =>
      props.session.arrangement.audioClips.filter((clip) =>
        props.selectedAudioClipIds.includes(clip.id),
      ),
    [props.selectedAudioClipIds, props.session.arrangement.audioClips],
  );
  const midiClips = useMemo(
    () =>
      props.session.arrangement.midiClips.filter((clip) =>
        props.selectedMidiClipIds.includes(clip.id),
      ),
    [props.selectedMidiClipIds, props.session.arrangement.midiClips],
  );
  const selectedClips = [...audioClips, ...midiClips];

  const run = (operation: Promise<ArrangementMutationResult | null>, onSuccess?: () => void) => {
    setMessage(null);
    void operation
      .then((next) => {
        if (!next) {
          setMessage('The edit was not applied.');
          return;
        }
        applyArrangementMutation(next, props.setSession, setMessage);
        onSuccess?.();
      })
      .catch((error: unknown) => {
        setMessage(error instanceof Error ? error.message : String(error));
      });
  };

  if (selectedClips.length < 2) return null;

  const duplicate = () => {
    const target = Math.max(
      ...selectedClips.map((clip) =>
        timelineObjectEndTick(clip, props.session.arrangement.timebase),
      ),
    );
    run(
      props.api.pasteTimelineClips(props.selectedAudioClipIds, props.selectedMidiClipIds, target),
    );
  };

  const summary = [audioClips.length, midiClips.length];
  return (
    <div className={styles.inspector}>
      <div className={styles.identity}>
        <span className={styles.identityIcon}>
          <Icon name="copy" />
        </span>
        <div className={styles.identityBlock}>
          <strong>{selectedClips.length} clips selected</strong>
          <small>
            {summary[0]} audio · {summary[1]} midi
          </small>
        </div>
      </div>
      <section className={styles.section}>
        <div className={styles.clipActions}>
          {selectedClips.length === 2 && audioClips.length === 2 && (
            <button
              type="button"
              className={clsx(styles.smallButton, styles.accent)}
              onClick={() => run(props.api.crossfadeAudioClips(audioClips[0].id, audioClips[1].id))}
            >
              Crossfade
            </button>
          )}
          <button type="button" className={styles.smallButton} onClick={duplicate}>
            Duplicate
          </button>
          <button
            type="button"
            className={clsx(styles.smallButton, styles.danger)}
            onClick={() =>
              run(
                props.api.removeTimelineClips(
                  props.selectedAudioClipIds,
                  props.selectedMidiClipIds,
                ),
                () => props.setSelectedClipIds([]),
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
