import type { CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { formatMusicalPosition } from '@/lib/arrange-timeline';
import styles from './ArrangeClipInspector.module.css';

interface ArrangeClipInspectorProps {
  session: CreativeSession;
  setSession: (session: CreativeSession) => void;
  selectedClipIds: string[];
  setSelectedClipIds: (ids: string[]) => void;
  api: NativeApi;
}

export function ArrangeClipInspector(props: ArrangeClipInspectorProps) {
  const selected = props.session.arrangement.audioClips.filter((clip) =>
    props.selectedClipIds.includes(clip.id),
  );
  const clip = selected.at(-1) ?? null;
  const commit = async (operation: Promise<CreativeSession | null>) => {
    const session = await operation;
    if (session) props.setSession(session);
  };

  if (!clip) {
    return (
      <div className={styles.empty}>
        <span className={styles.emptyGlyph}>⌁</span>
        <strong>No clip selected</strong>
        <p>Select an Audio Clip to edit its timing, source range and fades.</p>
      </div>
    );
  }

  const seconds = clip.timelineDuration.frames / clip.timelineDuration.sampleRate;
  const fadeInMs = (clip.fadeIn.frames * 1000) / clip.sourceSampleRate;
  const fadeOutMs = (clip.fadeOut.frames * 1000) / clip.sourceSampleRate;
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
            defaultValue={clip.name}
            key={`${clip.id}:${clip.name}`}
            onBlur={(event) => {
              const name = event.currentTarget.value.trim();
              if (name && name !== clip.name)
                void commit(props.api.updateAudioClip(clip.id, { name }));
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
            defaultValue={clip.startTick}
            key={`${clip.id}:start:${clip.startTick}`}
            onBlur={(event) =>
              void commit(
                props.api.updateAudioClip(clip.id, {
                  startTick: Number(event.currentTarget.value),
                }),
              )
            }
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
            defaultValue={clip.gainDb}
            key={`${clip.id}:gain:${clip.gainDb}`}
            onPointerUp={(event) =>
              void commit(
                props.api.updateAudioClip(clip.id, { gainDb: Number(event.currentTarget.value) }),
              )
            }
          />
          <output>{clip.gainDb.toFixed(1)} dB</output>
        </label>
        <label>
          <span>Pan</span>
          <input
            aria-label="Clip pan"
            type="range"
            min="-1"
            max="1"
            step="0.05"
            defaultValue={clip.pan}
            key={`${clip.id}:pan:${clip.pan}`}
            onPointerUp={(event) =>
              void commit(
                props.api.updateAudioClip(clip.id, { pan: Number(event.currentTarget.value) }),
              )
            }
          />
          <output>
            {Math.abs(clip.pan) < 0.01
              ? 'Center'
              : `${clip.pan < 0 ? 'L' : 'R'} ${Math.round(Math.abs(clip.pan) * 100)}`}
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
            defaultValue={Math.round(fadeInMs)}
            key={`${clip.id}:fadeIn:${clip.fadeIn.frames}`}
            onBlur={(event) =>
              void commit(
                props.api.updateAudioClip(clip.id, {
                  fadeIn: {
                    frames: Math.round(
                      (Number(event.currentTarget.value) * clip.sourceSampleRate) / 1000,
                    ),
                    sampleRate: clip.sourceSampleRate,
                  },
                }),
              )
            }
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
            defaultValue={Math.round(fadeOutMs)}
            key={`${clip.id}:fadeOut:${clip.fadeOut.frames}`}
            onBlur={(event) =>
              void commit(
                props.api.updateAudioClip(clip.id, {
                  fadeOut: {
                    frames: Math.round(
                      (Number(event.currentTarget.value) * clip.sourceSampleRate) / 1000,
                    ),
                    sampleRate: clip.sourceSampleRate,
                  },
                }),
              )
            }
          />
          <output>ms</output>
        </label>
      </section>

      <div className={styles.toggles}>
        <button
          className={clip.muted ? styles.active : ''}
          onClick={() => void commit(props.api.updateAudioClip(clip.id, { muted: !clip.muted }))}
        >
          Mute
        </button>
        <button
          className={clip.loopEnabled ? styles.active : ''}
          onClick={() =>
            void commit(props.api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }))
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
              void commit(props.api.crossfadeAudioClips(selected[0].id, selected[1].id))
            }
          >
            Create crossfade
          </button>
        )}
        <button onClick={() => void commit(props.api.duplicateAudioClip(clip.id))}>
          Duplicate
        </button>
        <button
          className={styles.danger}
          onClick={() =>
            void commit(props.api.removeAudioClips(props.selectedClipIds)).then(() =>
              props.setSelectedClipIds([]),
            )
          }
        >
          Delete
        </button>
      </div>
    </div>
  );
}
