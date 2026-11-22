import type { Rig } from '@/model/generated';
import { Icon } from '@/shared/ui/Icon';
import { inputLabel } from './model/input-label';
import styles from './RigBar.module.css';
import shared from './RigControl.module.css';

interface RigBarProps {
  /** The playing rig, or `null` while no rig plays. */
  active: Rig | null;
  open: boolean;
  onToggle: () => void;
  onStop: () => void;
  onOpenEditor: () => void;
}

/** The global-bar control: the playing rig with its actions, or the entry point. */
export function RigBar({ active, open, onToggle, onStop, onOpenEditor }: RigBarProps) {
  if (!active) {
    return (
      <button
        type="button"
        className={styles.stopped}
        aria-label="Rig"
        aria-expanded={open}
        onClick={onToggle}
      >
        <Icon name="mixer" />
        <span>Rig</span>
        <Icon name="chevron" />
      </button>
    );
  }
  return (
    <div className={styles.playing}>
      <button
        type="button"
        className={styles.summary}
        aria-label={`Rig: ${active.name}`}
        aria-expanded={open}
        onClick={onToggle}
      >
        <span className={styles.summaryText}>
          <strong className={shared.rigName} title={active.name}>
            {active.name}
          </strong>
          <small className={shared.rigInput}>{inputLabel(active.audioInput.channelIndex)}</small>
        </span>
      </button>
      <button
        type="button"
        className={styles.toggle}
        aria-label="Open rig list"
        title="Rig list"
        aria-expanded={open}
        onClick={onToggle}
      >
        <Icon name="chevron" />
      </button>
      <button
        type="button"
        aria-label="Open rig effect editor"
        title="Open effect editor"
        disabled={
          !active.effects.some((effect) => !effect.bypassed && !effect.plugin.disabledPlaceholder)
        }
        onClick={onOpenEditor}
      >
        <Icon name="expand" />
      </button>
      <button type="button" aria-label="Stop rig" title="Stop rig" onClick={onStop}>
        <Icon name="stop" />
      </button>
    </div>
  );
}
