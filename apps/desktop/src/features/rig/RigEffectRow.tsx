import type { EffectDevice } from '@/model/generated';
import { Icon } from '@/shared/ui/Icon';
import controls from '@/shared/ui/controls.module.css';
import styles from './RigEffectRow.module.css';

interface RigEffectRowProps {
  effect: EffectDevice;
  /** Whether the effect is the first of the chain and cannot move up. */
  first: boolean;
  /** Whether the effect is the last of the chain and cannot move down. */
  last: boolean;
  onBypass: () => void;
  onOpenEditor: () => void;
  /** Moves the effect by a signed offset in the chain. */
  onMove: (offset: number) => void;
  onRemove: () => void;
}

/** One effect of a rig: name, bypass and editor, with order and removal on hover. */
export function RigEffectRow(props: RigEffectRowProps) {
  const { effect } = props;
  return (
    <div className={styles.effect} data-bypassed={effect.bypassed}>
      <span className={styles.name} title={effect.name}>
        {effect.name}
      </span>
      <span className={styles.tools}>
        <button
          type="button"
          className={controls.iconButton}
          aria-label={`Move ${effect.name} up`}
          title="Move up"
          disabled={props.first}
          onClick={() => props.onMove(-1)}
        >
          <Icon name="chevron" className={styles.up} />
        </button>
        <button
          type="button"
          className={controls.iconButton}
          aria-label={`Move ${effect.name} down`}
          title="Move down"
          disabled={props.last}
          onClick={() => props.onMove(1)}
        >
          <Icon name="chevron" className={styles.down} />
        </button>
        <button
          type="button"
          className={`${controls.iconButton} ${controls.danger}`}
          aria-label={`Remove ${effect.name}`}
          title="Remove"
          onClick={props.onRemove}
        >
          <Icon name="trash" />
        </button>
      </span>
      <button
        type="button"
        className={controls.bypassButton}
        aria-pressed={effect.bypassed}
        aria-label={`Bypass ${effect.name}`}
        onClick={props.onBypass}
      >
        Bypass
      </button>
      <button
        type="button"
        className={controls.iconButton}
        aria-label={`Open ${effect.name} editor`}
        title="Open plug-in editor"
        disabled={effect.bypassed || effect.plugin.disabledPlaceholder}
        onClick={props.onOpenEditor}
      >
        <Icon name="maximize" />
      </button>
    </div>
  );
}
