import { clipDurationTicks } from '@/features/arrange/model/arrange-timeline';
import { formatMusicalLength } from '@/model/time-format';
import type { ArrangementMutationResult, AudioClip, CreativeSession } from '@/model/generated';
import { formatGainDb, formatPan } from '@/shared/audio/mix-format';
import { useSessionContext } from '@/shared/session/session-context';
import type { ContextMenuItem } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/primitives';
import clsx from 'clsx';
import { useEffect, useState, type CSSProperties } from 'react';
import { resolveTrackColorById } from '../model/track-colors';
import { ClipToggles } from './ClipToggles';
import styles from './Inspector.module.css';
import { InspectorMenu } from './InspectorMenu';
import { MixValueField } from './MixValueField';
import { MusicalTimeField } from './MusicalTimeField';

interface ArrangeClipInspectorProps {
  session: CreativeSession;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
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
  const { api, commit: sessionCommit } = useSessionContext();
  const selected = props.session.arrangement.audioClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.length === 1 ? selected[0] : null;
  const [drafts, setDrafts] = useState<Drafts | null>(clip ? buildDrafts(clip) : null);
  const [message, setMessage] = useState<string | null>(null);

  // Re-seed drafts when the selected clip identity changes. We do NOT reseed
  // on every value change, so the user can finish typing before a blur fires
  // even if the canonical session updates from another source.
  useEffect(() => {
    setMessage(null);
    if (clip) setDrafts(buildDrafts(clip));
    else setDrafts(null);
  }, [clip?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const commit = (operation: Promise<ArrangementMutationResult>, afterSuccess?: () => void) => {
    setMessage(null);
    void sessionCommit(operation, { onError: setMessage }).then((result) => {
      if (result) afterSuccess?.();
    });
  };

  if (!clip || !drafts) {
    return null;
  }

  const seconds = clip.timelineDuration.frames / clip.timelineDuration.sampleRate;
  const recordingTake = clip.recordingTakeId
    ? props.session.arrangement.takes.find((take) => take.id === clip.recordingTakeId)
    : undefined;
  const patch = (fields: Record<string, unknown>) =>
    void commit(api.updateAudioClip(clip.id, fields));

  const tracks = props.session.arrangement.tracks;
  const trackName = tracks.find((track) => track.id === clip.trackId)?.name;

  const menuItems: ContextMenuItem[] = [
    {
      label: 'Duplicate',
      onClick: () => commit(api.duplicateAudioClip(clip.id)),
    },
    ...(props.onSetLoopToClip
      ? [
          {
            label: 'Loop to Clip',
            onClick: () => {
              const operation = props.onSetLoopToClip?.(clip);
              if (operation) commit(operation);
            },
          },
        ]
      : []),
    { separator: true },
    {
      label: 'Delete',
      danger: true,
      onClick: () =>
        commit(api.removeTimelineClips([clip.id], []), () => props.setSelectedClipIds([])),
    },
  ];

  return (
    <div
      className={styles.inspector}
      style={{ '--track-color': resolveTrackColorById(tracks, clip.trackId) } as CSSProperties}
    >
      <div className={styles.identity}>
        <span className={styles.identityTile}>
          <Icon name="wave" />
        </span>
        <div className={styles.identityBlock}>
          <input
            className={styles.identityName}
            aria-label="Clip name"
            value={drafts.name}
            onChange={(event) => setDrafts({ ...drafts, name: event.currentTarget.value })}
            onBlur={() => {
              const name = drafts.name.trim();
              if (name && name !== clip.name) patch({ name });
            }}
          />
          <small>Audio clip{trackName && ` · ${trackName}`}</small>
        </div>
        <div className={styles.identityTools}>
          <ClipToggles
            muted={clip.muted}
            loopEnabled={clip.loopEnabled}
            onToggleMuted={() => commit(api.updateAudioClip(clip.id, { muted: !clip.muted }))}
            onToggleLoop={() =>
              commit(api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }))
            }
          />
          <InspectorMenu label="Clip actions" items={menuItems} />
        </div>
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
            onCommit={(startTick) => patch({ startTick })}
          />
          <div className={clsx(styles.inlineField, styles.readonly)}>
            <span>Length</span>
            <output>
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
                onClick={() => void commit(api.setAudioClipTakeVariant(clip.id, variant))}
              >
                {variant === 'raw' ? 'Raw' : 'Processed'}
              </button>
            ))}
          </div>
        </section>
      )}

      <section className={styles.section} aria-label="Clip mix">
        <div className={styles.rows}>
          <MixValueField
            label="Gain"
            name="Clip gain"
            value={clip.gainDb}
            min={-60}
            max={24}
            origin={-60}
            step={0.5}
            inputStep={0.1}
            format={formatGainDb}
            onCommit={(gainDb) => patch({ gainDb })}
          />
          <MixValueField
            label="Pan"
            name="Clip pan"
            value={clip.pan}
            min={-1}
            max={1}
            origin={0}
            step={0.05}
            inputStep={0.05}
            format={formatPan}
            onCommit={(pan) => patch({ pan })}
          />
        </div>
      </section>

      <section className={styles.section}>
        <header className={styles.sectionHeader}>
          <strong>FADES</strong>
        </header>
        <div className={styles.fieldPair}>
          <label className={styles.inlineField}>
            <span>In</span>
            <input
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
                  patch({ fadeIn: { frames, sampleRate: clip.sourceSampleRate } });
              }}
            />
            <small>ms</small>
          </label>
          <label className={styles.inlineField}>
            <span>Out</span>
            <input
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
                  patch({ fadeOut: { frames, sampleRate: clip.sourceSampleRate } });
              }}
            />
            <small>ms</small>
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
              onClick={() => patch({ fadeShape: shape })}
            >
              {shape === 'linear' ? 'Linear' : shape === 'equalPower' ? 'Equal' : 'Smooth'}
            </button>
          ))}
        </div>
      </section>

      {message && <p className={styles.message}>{message}</p>}
    </div>
  );
}
