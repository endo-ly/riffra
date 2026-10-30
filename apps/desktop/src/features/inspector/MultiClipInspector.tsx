import { timelineObjectEndTick } from '@/model/clip-time';
import type { ArrangementMutationResult, CreativeSession } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { Icon } from '@/shared/ui/primitives';
import clsx from 'clsx';
import { useMemo, useState } from 'react';
import sharedStyles from './Inspector.module.css';
import styles from './MultiClipInspector.module.css';

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
    <div className={sharedStyles.inspector}>
      <div className={sharedStyles.identity}>
        <span className={sharedStyles.identityTile}>
          <Icon name="copy" />
        </span>
        <div className={sharedStyles.identityBlock}>
          <strong>{selectedClips.length} clips selected</strong>
          <small>
            {summary[0]} audio · {summary[1]} midi
          </small>
        </div>
      </div>
      <section className={sharedStyles.section}>
        <div className={styles.buttonRow}>
          {selectedClips.length === 2 && audioClips.length === 2 && (
            <button
              type="button"
              className={clsx(sharedStyles.smallButton, sharedStyles.accent)}
              onClick={() => run(api.crossfadeAudioClips(audioClips[0].id, audioClips[1].id))}
            >
              Crossfade
            </button>
          )}
          <button type="button" className={sharedStyles.smallButton} onClick={duplicate}>
            Duplicate
          </button>
          <button
            type="button"
            className={clsx(sharedStyles.smallButton, styles.danger)}
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
      {message && <p className={sharedStyles.message}>{message}</p>}
    </div>
  );
}
