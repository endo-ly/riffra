import { useMemo, useState } from 'react';
import type { CreativeSession } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { timelineObjectEndTick } from '@/features/arrange/model/arrange-timeline';
import styles from './ArrangeClipInspector.module.css';

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

  const run = (operation: Promise<CreativeSession | null>, onSuccess?: () => void) => {
    setMessage(null);
    void operation
      .then((next) => {
        if (!next) {
          setMessage('The edit was not applied.');
          return;
        }
        props.setSession(next);
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

  return (
    <div className={styles.inspector}>
      <div className={styles.actions}>
        {selectedClips.length === 2 && audioClips.length === 2 && (
          <button
            className={styles.primary}
            onClick={() => run(props.api.crossfadeAudioClips(audioClips[0].id, audioClips[1].id))}
          >
            Crossfade
          </button>
        )}
        <button onClick={duplicate}>Duplicate</button>
        <button
          className={styles.danger}
          onClick={() =>
            run(
              props.api.removeTimelineClips(props.selectedAudioClipIds, props.selectedMidiClipIds),
              () => props.setSelectedClipIds([]),
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
