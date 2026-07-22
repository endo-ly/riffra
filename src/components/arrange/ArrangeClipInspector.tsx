import { useEffect, useState } from 'react';
import type { AudioClip, CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { clipDurationTicks, formatMusicalPosition } from '@/lib/arrange-timeline';
import styles from './ArrangeClipInspector.module.css';

interface ArrangeClipInspectorProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: NativeApi;
  onSetLoopToClip?: (clip: AudioClip) => void;
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

export function ArrangeClipInspector(props: ArrangeClipInspectorProps) {
  const selected = props.session.arrangement.audioClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.at(-1) ?? null;
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

  const commit = async (operation: Promise<CreativeSession | null>, label: string) => {
    try {
      const next = await operation;
      if (next) {
        props.setSession(next);
        setMessage(`${label} applied.`);
      } else {
        setMessage(`${label} was not applied.`);
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  if (!clip || !drafts) {
    return (
      <div className={styles.empty}>
        <span className={styles.emptyGlyph}>⌁</span>
        <strong>No clip selected</strong>
        <p>Select an Audio Clip to edit its timing, source range and fades.</p>
      </div>
    );
  }

  const seconds = clip.timelineDuration.frames / clip.timelineDuration.sampleRate;
  const patch = (fields: Record<string, unknown>, label: string) =>
    void commit(props.api.updateAudioClip(clip.id, fields), label);

  const duplicateSelection = () => {
    // Keep Duplicate consistent with Delete and Ctrl+D: operate on the full
    // selection rather than just the last-focused clip.
    const clips = selected;
    const target = Math.max(
      ...clips.map(
        (item) => item.startTick + clipDurationTicks(item, props.session.arrangement.timebase),
      ),
    );
    void commit(
      props.api.pasteAudioClips(props.selectedClipIds, target),
      `${selected.length} clip${selected.length === 1 ? '' : 's'} duplicated.`,
    );
  };

  return (
    <div className={styles.inspector}>
      <section className={styles.identity}>
        <span className={styles.art}>▥</span>
        <div>
          <span className="eyebrow">
            {selected.length > 1 ? `${selected.length} AUDIO CLIPS` : 'AUDIO CLIP'}
          </span>
          <input
            aria-label="Clip name"
            value={drafts.name}
            onChange={(event) => setDrafts({ ...drafts, name: event.currentTarget.value })}
            onBlur={() => {
              const name = drafts.name.trim();
              if (name && name !== clip.name) patch({ name }, 'Rename');
            }}
          />
          <small>{clip.assetId}</small>
        </div>
      </section>

      <section className={styles.section}>
        <header>
          <strong>Timing</strong>
          <span>{formatMusicalPosition(clip.startTick, props.session.arrangement.timebase)}</span>
        </header>
        <label>
          <span>Start tick</span>
          <input
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
        <div className={styles.readout}>
          <span>Length</span>
          <strong>{seconds.toFixed(3)} s</strong>
        </div>
        <div className={styles.readout}>
          <span>Source</span>
          <strong>
            {clip.sourceRange.start.toLocaleString()} – {clip.sourceRange.end.toLocaleString()}
          </strong>
        </div>
      </section>

      <section className={styles.section}>
        <header>
          <strong>Clip mix</strong>
          <span>PRE-RACK</span>
        </header>
        <label>
          <span>Gain</span>
          <input
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
          <output>{Number(drafts.gainDb).toFixed(1)} dB</output>
        </label>
        <label>
          <span>Pan</span>
          <input
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
          <output>
            {Math.abs(Number(drafts.pan)) < 0.01
              ? 'Center'
              : `${Number(drafts.pan) < 0 ? 'L' : 'R'} ${Math.round(Math.abs(Number(drafts.pan)) * 100)}`}
          </output>
        </label>
      </section>

      <section className={styles.section}>
        <header>
          <strong>Fades</strong>
          <span>EQUAL POWER</span>
        </header>
        <label>
          <span>Fade in</span>
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
                patch({ fadeIn: { frames, sampleRate: clip.sourceSampleRate } }, 'Fade in');
            }}
          />
          <output>ms</output>
        </label>
        <label>
          <span>Fade out</span>
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
                patch({ fadeOut: { frames, sampleRate: clip.sourceSampleRate } }, 'Fade out');
            }}
          />
          <output>ms</output>
        </label>
      </section>

      <div className={styles.toggles}>
        <button
          className={clip.muted ? styles.active : ''}
          onClick={() =>
            void commit(props.api.updateAudioClip(clip.id, { muted: !clip.muted }), 'Mute')
          }
        >
          Mute
        </button>
        <button
          className={clip.loopEnabled ? styles.active : ''}
          onClick={() =>
            void commit(
              props.api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }),
              'Loop',
            )
          }
        >
          Loop
        </button>
      </div>

      <div className={styles.actions}>
        {selected.length === 2 && (
          <button
            className={styles.primary}
            onClick={() =>
              void commit(
                props.api.crossfadeAudioClips(selected[0].id, selected[1].id),
                'Crossfade',
              )
            }
          >
            Create crossfade
          </button>
        )}
        {props.onSetLoopToClip && selected.length === 1 && (
          <button onClick={() => props.onSetLoopToClip?.(clip)}>Set Loop to Clip</button>
        )}
        <button onClick={duplicateSelection}>
          Duplicate{selected.length > 1 ? ` (${selected.length})` : ''}
        </button>
        <button
          className={styles.danger}
          onClick={() =>
            void commit(props.api.removeAudioClips(props.selectedClipIds), 'Delete').then(() =>
              props.setSelectedClipIds([]),
            )
          }
        >
          Delete{selected.length > 1 ? ` (${selected.length})` : ''}
        </button>
      </div>

      {message && <p className={styles.message}>{message}</p>}
    </div>
  );
}
