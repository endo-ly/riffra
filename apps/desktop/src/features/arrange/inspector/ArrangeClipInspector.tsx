import { useEffect, useState } from 'react';
import clsx from 'clsx';
import type { ArrangementMutationResult, AudioClip, CreativeSession } from '@/model/domain';
import type { ArrangeInspectorApi } from '../arrange-api';
import { formatMusicalPosition } from '@/features/arrange/model/arrange-timeline';
import { Icon } from '@/shared/ui/primitives';
import styles from './Inspector.module.css';
import { useInspectorOperation } from './useInspectorOperation';
import { applyArrangementMutation } from '@/shared/session/apply-arrangement-mutation';

interface ArrangeClipInspectorProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: ArrangeInspectorApi;
  onSetLoopToClip?: (clip: AudioClip) => Promise<ArrangementMutationResult>;
}

interface Drafts {
  name: string;
  startTick: string;
  gainDb: string;
  pan: string;
  fadeInMs: string;
  fadeOutMs: string;
}

function buildDrafts(clip: AudioClip): Drafts {
  const fadeInMs = (clip.fadeIn.frames * 1000) / clip.sourceSampleRate;
  const fadeOutMs = (clip.fadeOut.frames * 1000) / clip.sourceSampleRate;
  return {
    name: clip.name,
    startTick: String(clip.startTick),
    gainDb: clip.gainDb.toFixed(1),
    pan: clip.pan.toFixed(2),
    fadeInMs: String(Math.round(fadeInMs)),
    fadeOutMs: String(Math.round(fadeOutMs)),
  };
}

function formatPan(pan: number) {
  if (Math.abs(pan) < 0.01) return 'C';
  return `${pan < 0 ? 'L' : 'R'} ${Math.round(Math.abs(pan) * 100)}`;
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
        applyArrangementMutation(next, props.setSession, setMessage);
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
        <span className={styles.identityMeta}>
          {formatMusicalPosition(clip.startTick, props.session.arrangement.timebase)}
        </span>
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
          <label className={styles.field}>
            <span>Start</span>
            <input
              className={clsx(styles.control, styles.mono)}
              type="number"
              min="0"
              value={drafts.startTick}
              onChange={(event) => setDrafts({ ...drafts, startTick: event.currentTarget.value })}
              onBlur={() => {
                const next = Number(drafts.startTick);
                if (Number.isFinite(next) && next >= 0 && next !== clip.startTick)
                  patch({ startTick: next }, 'Start tick');
              }}
            />
          </label>
          <div className={styles.readoutRow}>
            <span>Pos</span>
            <strong>
              {formatMusicalPosition(clip.startTick, props.session.arrangement.timebase)}
            </strong>
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
        <label className={styles.mixField}>
          <span>
            Gain <span className={styles.value}>{Number(drafts.gainDb).toFixed(1)} dB</span>
          </span>
          <input
            className={styles.range}
            aria-label="Clip gain"
            type="range"
            min="-60"
            max="24"
            step="0.5"
            value={drafts.gainDb}
            onChange={(event) => setDrafts({ ...drafts, gainDb: event.currentTarget.value })}
            onPointerUp={() => {
              const next = Number(drafts.gainDb);
              if (Number.isFinite(next) && next !== clip.gainDb) patch({ gainDb: next }, 'Gain');
            }}
          />
        </label>
        <label className={styles.mixField}>
          <span>
            Pan <span className={styles.value}>{formatPan(Number(drafts.pan))}</span>
          </span>
          <input
            className={styles.range}
            aria-label="Clip pan"
            type="range"
            min="-1"
            max="1"
            step="0.05"
            value={drafts.pan}
            onChange={(event) => setDrafts({ ...drafts, pan: event.currentTarget.value })}
            onPointerUp={() => {
              const next = Number(drafts.pan);
              if (Number.isFinite(next) && next !== clip.pan) patch({ pan: next }, 'Pan');
            }}
          />
        </label>
      </div>

      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>FADES</strong>
          <span className={styles.headerMeta}>EQUAL POWER</span>
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
      </section>

      <section className={styles.section}>
        <div className={styles.clipActions}>
          <div className={styles.segmented} role="group" aria-label="Clip state">
            <button
              type="button"
              aria-pressed={clip.muted}
              onClick={() =>
                commit(props.api.updateAudioClip(clip.id, { muted: !clip.muted }), 'Mute')
              }
            >
              Mute
            </button>
            <button
              type="button"
              aria-pressed={clip.loopEnabled}
              onClick={() =>
                commit(
                  props.api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }),
                  'Loop',
                )
              }
            >
              Loop
            </button>
          </div>
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
