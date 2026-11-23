import type { Rig } from '@/model/generated';
import { Icon } from '@/shared/ui/Icon';
import styles from './RigTrigger.module.css';

interface RigTriggerProps {
  /** The rig the bar plays and stops: the playing one, else the one chosen last. */
  rig: Rig | null;
  playing: boolean;
  open: boolean;
  onToggle: () => void;
  onPlay: () => void;
  onStop: () => void;
}

/**
 * The global-bar entry: the rig's name opens the panel, and the button joined
 * to it plays or stops that rig.
 */
export function RigTrigger({ rig, playing, open, onToggle, onPlay, onStop }: RigTriggerProps) {
  return (
    <div className={styles.group} data-joined={rig !== null}>
      <button
        type="button"
        className={styles.trigger}
        aria-label={rig ? `Rig: ${rig.name}` : 'Rig'}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={onToggle}
      >
        {playing ? (
          <span className={styles.playingDot} aria-hidden="true" />
        ) : (
          <Icon name="mixer" />
        )}
        <span className={styles.name} title={rig?.name}>
          {rig?.name ?? 'Rig'}
        </span>
        <Icon name="chevron" className={styles.chevron} />
      </button>
      {rig && (
        <button
          type="button"
          className={styles.playback}
          aria-label={`${playing ? 'Stop' : 'Play'} rig ${rig.name}`}
          title={playing ? 'Stop rig' : 'Play rig'}
          onClick={playing ? onStop : onPlay}
        >
          <Icon name={playing ? 'stop' : 'play'} />
        </button>
      )}
    </div>
  );
}
