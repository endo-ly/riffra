import { Icon } from '@/shared/ui/primitives';
import styles from './Inspector.module.css';

interface ClipTogglesProps {
  muted: boolean;
  loopEnabled: boolean;
  onToggleMuted: () => void;
  onToggleLoop: () => void;
}

/** Mute and Loop of a clip as icon toggles for the inspector header. */
export function ClipToggles(props: ClipTogglesProps) {
  return (
    <>
      <button
        type="button"
        className={styles.iconButton}
        aria-label="Mute clip"
        aria-pressed={props.muted}
        title={props.muted ? 'Unmute clip' : 'Mute clip'}
        onClick={props.onToggleMuted}
      >
        <Icon name={props.muted ? 'speakerOff' : 'speaker'} />
      </button>
      <button
        type="button"
        className={styles.iconButton}
        aria-label="Loop clip"
        aria-pressed={props.loopEnabled}
        title={props.loopEnabled ? 'Stop looping clip' : 'Loop clip'}
        onClick={props.onToggleLoop}
      >
        <Icon name="loop" />
      </button>
    </>
  );
}
