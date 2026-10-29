import { timelineObjectEndTick } from '@/features/arrange/model/arrange-timeline';
import type { ArrangementMutationResult, CreativeSession } from '@/model/domain';
import { useSessionContext } from '@/shared/session/session-context';
import { Icon } from '@/shared/ui/primitives';
import clsx from 'clsx';
import { useMemo, useState } from 'react';
import styles from './Inspector.module.css';

interface MultiClipInspectorProps {
  session: CreativeSession;
  selectedAudioClipIds: string[];
  selectedMidiClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
}

export function MultiClipInspector(props: MultiClipInspectorProps) {
  const { api, commit: sessionCommit } = useSessionContext();
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

  const run = (operation: Promise<ArrangementMutationResult>, onSuccess?: () => void) => {
    setMessage(null);
    void sessionCommit(operation, { onError: setMessage }).then((result) => {
      if (result) onSuccess?.();
    });
  };

  if (selectedClips.length < 2) return null;

  const duplicate = () => {
    const target = Math.max(
      ...selectedClips.map((clip) =>
        timelineObjectEndTick(clip, props.session.arrangement.timebase),
      ),
    );
    run(api.pasteTimelineClips(props.selectedAudioClipIds, props.selectedMidiClipIds, target));
  };

  const summary = [audioClips.length, midiClips.length];
  return (
    <div className={styles.inspector}>
      <div className={styles.identity}>
        <span className={styles.identityTile}>
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
        <div className={styles.buttonRow}>
          {selectedClips.length === 2 && audioClips.length === 2 && (
            <button
              type="button"
              className={clsx(styles.smallButton, styles.accent)}
              onClick={() => run(api.crossfadeAudioClips(audioClips[0].id, audioClips[1].id))}
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
                api.removeTimelineClips(props.selectedAudioClipIds, props.selectedMidiClipIds),
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
