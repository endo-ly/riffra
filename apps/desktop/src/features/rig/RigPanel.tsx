import type { Rig, Track } from '@/model/generated';
import { useSession } from '@/session/session-context';
import { Icon } from '@/shared/ui/Icon';
import { inputLabel } from './model/input-label';
import { RigEffectRow } from './RigEffectRow';
import shared from './RigControl.module.css';
import styles from './RigPanel.module.css';

/** The track that monitors the same input as the rig, if any. */
function monitoringTrack(tracks: Track[], rig: Rig): Track | undefined {
  return tracks.find(
    (track) =>
      track.kind === 'audio' &&
      track.audioInput?.channelIndex === rig.audioInput.channelIndex &&
      (track.monitoring === 'on' || (track.monitoring === 'auto' && track.armed)),
  );
}

interface RigPanelProps {
  /** The new-rig name draft, kept while the panel is closed. */
  newName: string;
  onNewNameChange: (name: string) => void;
  onStart: (rigId: string) => void;
  onStop: () => void;
  onPickEffect: (rigId: string) => void;
  onDelete: (rig: Rig) => void;
}

/** The `▾` panel: every rig, the playing rig's settings, and the new-rig entry. */
export function RigPanel(props: RigPanelProps) {
  const rig = useSession((state) => state.rig);
  const tracks = useSession((state) => state.project.boot!.canonical.session.arrangement.tracks);
  const inputs = useSession((state) => state.audio.status.inputChannels);
  const create = () => {
    const name = props.newName.trim() || `Rig ${rig.state.rigs.length + 1}`;
    void rig.create(name, inputs[0]?.index ?? 0).then((created) => {
      if (created) props.onNewNameChange('');
    });
  };

  return (
    <div className={styles.panel} role="dialog" aria-label="Rigs">
      <div className={styles.list} aria-label="Rig list">
        {rig.state.rigs.length === 0 && <p className={styles.empty}>No rigs yet.</p>}
        {rig.state.rigs.map((item) => {
          const activeItem = item.id === rig.state.activeRigId;
          const monitoredBy = activeItem ? monitoringTrack(tracks, item) : undefined;
          return (
            <section
              key={item.id}
              className={styles.item}
              data-active={activeItem}
              aria-label={item.name}
            >
              <div className={styles.itemRow}>
                <button
                  type="button"
                  className={styles.itemName}
                  aria-pressed={activeItem}
                  onClick={() => (activeItem ? props.onStop() : props.onStart(item.id))}
                >
                  <strong className={shared.rigName} title={item.name}>
                    {item.name}
                  </strong>
                  <small className={shared.rigInput}>
                    {inputLabel(item.audioInput.channelIndex)}
                  </small>
                </button>
                {activeItem ? (
                  <span className={styles.playingDot} aria-label="Playing" />
                ) : (
                  <Icon name="play" className={styles.playIcon} />
                )}
              </div>
              {activeItem && (
                <div className={styles.detail}>
                  <label className={styles.field}>
                    <span>Name</span>
                    <input
                      key={`${item.id}:${item.name}`}
                      defaultValue={item.name}
                      onBlur={(event) => {
                        const name = event.currentTarget.value.trim();
                        if (name && name !== item.name) void rig.update(item.id, { name });
                      }}
                    />
                  </label>
                  <label className={styles.field}>
                    <span>Input</span>
                    <select
                      value={item.audioInput.channelIndex}
                      onChange={(event) =>
                        void rig.update(item.id, {
                          channelIndex: Number(event.currentTarget.value),
                        })
                      }
                    >
                      {Array.from(
                        new Set([
                          item.audioInput.channelIndex,
                          ...inputs.map((input) => input.index),
                        ]),
                      )
                        .sort((left, right) => left - right)
                        .map((index) => (
                          <option key={index} value={index}>
                            {inputLabel(index)}
                          </option>
                        ))}
                    </select>
                  </label>
                  <label className={styles.field}>
                    <span>Volume</span>
                    <input
                      key={`${item.id}:${item.gainDb}`}
                      type="number"
                      step="0.1"
                      defaultValue={item.gainDb}
                      onBlur={(event) => {
                        const gainDb = Number(event.currentTarget.value);
                        if (Number.isFinite(gainDb) && gainDb !== item.gainDb)
                          void rig.update(item.id, { gainDb });
                      }}
                    />
                    <span>dB</span>
                  </label>
                  {monitoredBy && (
                    <p className={styles.warning}>
                      {inputLabel(item.audioInput.channelIndex)} is also monitored by track &quot;
                      {monitoredBy.name}&quot;.
                    </p>
                  )}
                  <div className={styles.effects} aria-label="Rig effects">
                    {item.effects.map((effect, index) => (
                      <RigEffectRow
                        key={effect.id}
                        effect={effect}
                        first={index === 0}
                        last={index + 1 === item.effects.length}
                        onBypass={() =>
                          void rig.setEffectBypassed(item.id, effect.id, !effect.bypassed)
                        }
                        onOpenEditor={() => void rig.openEditor(item.id, effect.id)}
                        onMove={(offset) => {
                          const ids = item.effects.map((device) => device.id);
                          [ids[index + offset], ids[index]] = [ids[index], ids[index + offset]];
                          void rig.moveEffect(item.id, ids);
                        }}
                        onRemove={() => void rig.removeEffect(item.id, effect.id)}
                      />
                    ))}
                    <button
                      type="button"
                      className={styles.addEffect}
                      onClick={() => props.onPickEffect(item.id)}
                    >
                      <Icon name="plus" /> Add Effect
                    </button>
                  </div>
                  <div className={styles.actions}>
                    <button type="button" onClick={() => void rig.duplicate(item.id)}>
                      Duplicate
                    </button>
                    <button
                      type="button"
                      className={styles.danger}
                      onClick={() => props.onDelete(item)}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              )}
            </section>
          );
        })}
      </div>
      <div className={styles.newRig}>
        <input
          aria-label="New rig name"
          placeholder="New rig"
          value={props.newName}
          onChange={(event) => props.onNewNameChange(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') create();
          }}
        />
        <button type="button" onClick={create}>
          New rig
        </button>
      </div>
      {rig.error && <p className={styles.error}>{rig.error}</p>}
    </div>
  );
}
