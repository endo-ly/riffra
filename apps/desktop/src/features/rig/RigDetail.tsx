import type { Rig, Track } from '@/model/generated';
import { formatGainDb, MIX_GAIN_DB } from '@/model/mix';
import { useSession } from '@/session/session-context';
import { Icon } from '@/shared/ui/Icon';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';
import { ValueSlider } from '@/shared/ui/ValueSlider';
import { useRigGain } from './hooks/useRigGain';
import { inputChoices, inputLabel, monitoringTrack, movedEffectIds } from './model/rig';
import { RigEffectRow } from './RigEffectRow';
import styles from './RigDetail.module.css';

const NO_TRACKS: readonly Track[] = [];

interface RigDetailProps {
  rig: Rig;
  playing: boolean;
  onPickEffect: () => void;
}

/** One rig's signal path: input, effects in order, then volume. */
export function RigDetail({ rig, playing, onPickEffect }: RigDetailProps) {
  const rigStore = useSession((state) => state.rig);
  // A rig outlives the Project, so it is shown while no Project is loaded too.
  const tracks = useSession(
    (state) => state.project.boot?.canonical.session.arrangement.tracks ?? NO_TRACKS,
  );
  const inputs = useSession((state) => state.audio.status.inputChannels);
  const gain = useRigGain(rig.gainDb, (gainDb) => rigStore.update(rig.id, { gainDb }));
  const monitoredBy = playing ? monitoringTrack(tracks, rig) : undefined;

  return (
    <section className={styles.detail} aria-label={rig.name} data-playing={playing}>
      {monitoredBy && (
        <InlineMessage tone="warning">
          {inputLabel(rig.audioInput.channelIndex)} is also monitored by track &quot;
          {monitoredBy.name}&quot;.
        </InlineMessage>
      )}

      <ol className={styles.path} aria-label="Signal path">
        <li>
          <span className={styles.endpointLabel}>Input</span>
          <select
            className={styles.input}
            aria-label="Rig input"
            value={rig.audioInput.channelIndex}
            onChange={(event) =>
              void rigStore.update(rig.id, { channelIndex: Number(event.currentTarget.value) })
            }
          >
            {inputChoices(
              rig,
              inputs.map((input) => input.index),
            ).map((index) => (
              <option key={index} value={index}>
                {inputLabel(index)}
              </option>
            ))}
          </select>
        </li>
        {rig.effects.map((effect, index) => (
          <li key={effect.id} className={styles.stage} data-bypassed={effect.bypassed}>
            <RigEffectRow
              effect={effect}
              first={index === 0}
              last={index + 1 === rig.effects.length}
              onBypass={() => void rigStore.setEffectBypassed(rig.id, effect.id, !effect.bypassed)}
              onOpenEditor={() => void rigStore.openEditor(rig.id, effect.id)}
              onMove={(offset) =>
                void rigStore.moveEffect(rig.id, movedEffectIds(rig, index, offset))
              }
              onRemove={() => void rigStore.removeEffect(rig.id, effect.id)}
            />
          </li>
        ))}
        <li className={styles.add}>
          <button type="button" className={styles.addEffect} onClick={onPickEffect}>
            <Icon name="plus" /> Add Effect
          </button>
        </li>
        <li>
          <span className={styles.endpointLabel}>Volume</span>
          <ValueSlider
            control={gain}
            range={MIX_GAIN_DB}
            ariaLabel="Rig volume"
            fillOrigin={MIX_GAIN_DB.min}
          />
          <output className={styles.value}>{formatGainDb(gain.value)}</output>
        </li>
      </ol>
    </section>
  );
}
