import { useEffect, useState } from 'react';
import clsx from 'clsx';
import type {
  ArrangementMutationResult,
  AudioClip,
  CanonicalState,
  CreativeSession,
} from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { clipDurationTicks, formatMusicalLength } from '@/features/arrange/model/arrange-timeline';
import { Icon } from '@/shared/ui/primitives';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { MixValueField } from './MixValueField';
import { MusicalTimeField } from './MusicalTimeField';
import styles from './Inspector.module.css';
import { useInspectorOperation } from './useInspectorOperation';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface ArrangeClipInspectorProps {
  session: CreativeSession;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: ArrangeInspectorApi;
  onSetLoopToClip?: (clip: AudioClip) => Promise<ArrangementMutationResult>;
}

interface Drafts {
  name: string;
  fadeInMs: string;
  fadeOutMs: string;
}

function buildDrafts(clip: AudioClip): Drafts {
  const fadeInMs = (clip.fadeIn.frames * 1000) / clip.sourceSampleRate;
  const fadeOutMs = (clip.fadeOut.frames * 1000) / clip.sourceSampleRate;
  return {
    name: clip.name,
    fadeInMs: String(Math.round(fadeInMs)),
    fadeOutMs: String(Math.round(fadeOutMs)),
  };
}

export function ArrangeClipInspector(props: ArrangeClipInspectorProps) {
  const selected = props.session.arrangement.audioClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.length === 1 ? selected[0] : null;
  const [drafts, setDrafts] = useState<Drafts | null>(clip ? buildDrafts(clip) : null);
  const {
    operationMessage: message,
    runOperation,
    setOperationMessage: setMessage,
  } = useInspectorOperation();

  // Re-seed drafts when the selected clip identity changes. We do NOT reseed
  // on every value change, so the user can finish typing before a blur fires
  // even if the canonical session updates from another source.
  useEffect(() => {
    setMessage(null);
    if (clip) setDrafts(buildDrafts(clip));
    else setDrafts(null);
  }, [clip?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const commit = (
    operation: Promise<ArrangementMutationResult | null>,
    label: string,
    afterSuccess?: () => void,
  ) => {
    runOperation(operation, (next) => {
      if (next) {
        applyArrangementMutation(next, props.applyCanonicalState, setMessage);
        afterSuccess?.();
      } else {
        setMessage(`${label} was not applied.`);
      }
    });
  };

  if (!clip || !drafts) {
    return null;
  }

  const seconds = clip.timelineDuration.frames / clip.timelineDuration.sampleRate;
  const recordingTake = clip.recordingTakeId
    ? props.session.arrangement.takes.find((take) => take.id === clip.recordingTakeId)
    : undefined;
  const patch = (fields: Record<string, unknown>, label: string) =>
    void commit(props.api.updateAudioClip(clip.id, fields), label);

  return (
    <div className={styles.inspector}>
      <div className={styles.identity}>
        <span className={styles.identityIcon}>
          <Icon name="wave" />
        </span>
        <input
          className={styles.identityName}
          aria-label="Clip name"
          value={drafts.name}
          onChange={(event) => setDrafts({ ...drafts, name: event.currentTarget.value })}
          onBlur={() => {
            const name = drafts.name.trim();
            if (name && name !== clip.name) patch({ name }, 'Rename');
          }}
        />
      </div>

      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>TIMING</strong>
          <span className={styles.headerMeta}>
            {seconds.toFixed(3)} s · {clip.sourceRange.start.toLocaleString()}–
            {clip.sourceRange.end.toLocaleString()}
          </span>
        </header>
        <div className={styles.fieldPair}>
          <MusicalTimeField
            label="Start"
            kind="position"
            ticks={clip.startTick}
            timebase={props.session.arrangement.timebase}
            onCommit={(startTick) => patch({ startTick }, 'Start')}
          />
          <div className={styles.field}>
            <span>Length</span>
            <output className={clsx(styles.control, styles.mono, styles.readonly)}>
              {formatMusicalLength(
                clipDurationTicks(clip, props.session.arrangement.timebase),
                props.session.arrangement.timebase,
              )}
            </output>
          </div>
        </div>
      </section>

      {recordingTake?.rawAudio && recordingTake.processedAudio && (
        <section className={styles.section}>
          <header className={styles.sectionHeader}>
            <strong>SOURCE</strong>
            <span className={styles.headerMeta}>CLIP ONLY</span>
          </header>
          <div className={styles.segmented} role="group" aria-label="Clip recording source">
            {(['raw', 'processed'] as const).map((variant) => (
              <button
                key={variant}
                type="button"
                aria-pressed={clip.takeVariant === variant}
                onClick={() =>
                  void commit(
                    props.api.setAudioClipTakeVariant(clip.id, variant),
                    variant === 'raw' ? 'Raw source' : 'Processed source',
                  )
                }
              >
                {variant === 'raw' ? 'Raw' : 'Processed'}
              </button>
            ))}
          </div>
        </section>
      )}

      <div className={styles.mixCluster} aria-label="Clip mix">
        <MixValueField
          label="Gain"
          name="Clip gain"
          value={clip.gainDb}
          min={-60}
          max={24}
          step={0.5}
          inputStep={0.1}
          format={formatGainDb}
          onCommit={(gainDb) => patch({ gainDb }, 'Gain')}
        />
        <MixValueField
          label="Pan"
          name="Clip pan"
          value={clip.pan}
          min={-1}
          max={1}
          step={0.05}
          inputStep={0.05}
          format={formatPan}
          onCommit={(pan) => patch({ pan }, 'Pan')}
        />
      </div>

      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>FADES</strong>
        </header>
        <div className={styles.fieldPair}>
          <label className={styles.field}>
            <span>In</span>
            <input
              className={clsx(styles.control, styles.mono)}
              type="number"
              min="0"
              max={seconds * 1000}
              step="1"
              value={drafts.fadeInMs}
              onChange={(event) => setDrafts({ ...drafts, fadeInMs: event.currentTarget.value })}
              onBlur={() => {
                const ms = Number(drafts.fadeInMs);
                if (!Number.isFinite(ms) || ms < 0) return;
                const frames = Math.round((ms * clip.sourceSampleRate) / 1000);
                if (frames !== clip.fadeIn.frames)
                  patch({ fadeIn: { frames, sampleRate: clip.sourceSampleRate } }, 'Fade in');
              }}
            />
          </label>
          <label className={styles.field}>
            <span>Out</span>
            <input
              className={clsx(styles.control, styles.mono)}
              type="number"
              min="0"
              max={seconds * 1000}
              step="1"
              value={drafts.fadeOutMs}
              onChange={(event) => setDrafts({ ...drafts, fadeOutMs: event.currentTarget.value })}
              onBlur={() => {
                const ms = Number(drafts.fadeOutMs);
                if (!Number.isFinite(ms) || ms < 0) return;
                const frames = Math.round((ms * clip.sourceSampleRate) / 1000);
                if (frames !== clip.fadeOut.frames)
                  patch({ fadeOut: { frames, sampleRate: clip.sourceSampleRate } }, 'Fade out');
              }}
            />
          </label>
        </div>
        <div
          className={clsx(styles.segmented, styles.segmentedGap)}
          aria-label="Fade shape"
          role="group"
        >
          {(['linear', 'equalPower', 'smooth'] as const).map((shape) => (
            <button
              key={shape}
              type="button"
              aria-pressed={(clip.fadeShape ?? 'equalPower') === shape}
              onClick={() => patch({ fadeShape: shape }, 'Fade shape')}
            >
              {shape === 'linear' ? 'Linear' : shape === 'equalPower' ? 'Equal' : 'Smooth'}
            </button>
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.clipActions}>
          <button
            type="button"
            className={styles.smallButton}
            aria-pressed={clip.muted}
            onClick={() =>
              commit(props.api.updateAudioClip(clip.id, { muted: !clip.muted }), 'Mute')
            }
          >
            Mute
          </button>
          <button
            type="button"
            className={styles.smallButton}
            aria-pressed={clip.loopEnabled}
            onClick={() =>
              commit(props.api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }), 'Loop')
            }
          >
            Loop
          </button>
          <button
            type="button"
            className={styles.smallButton}
            onClick={() => commit(props.api.duplicateAudioClip(clip.id), 'Duplicate')}
          >
            Duplicate
          </button>
          {props.onSetLoopToClip && (
            <button
              type="button"
              className={styles.smallButton}
              onClick={() => {
                const operation = props.onSetLoopToClip?.(clip);
                if (operation) commit(operation, 'Loop range');
              }}
            >
              Loop to Clip
            </button>
          )}
          <button
            type="button"
            className={clsx(styles.smallButton, styles.danger)}
            onClick={() =>
              commit(props.api.removeTimelineClips([clip.id], []), 'Delete', () =>
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
