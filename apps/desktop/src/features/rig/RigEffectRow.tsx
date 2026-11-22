import type { EffectDevice } from '@/model/generated';
import { Icon } from '@/shared/ui/Icon';
import styles from './RigEffectRow.module.css';

interface RigEffectRowProps {
  effect: EffectDevice;
  /** Whether moving the effect towards the start of the chain is possible. */
  first: boolean;
  /** Whether moving the effect towards the end of the chain is possible. */
  last: boolean;
  onBypass: () => void;
  onOpenEditor: () => void;
  /** Moves the effect by a signed offset in the chain. */
  onMove: (offset: number) => void;
  onRemove: () => void;
}

/** One effect of the playing rig: name, bypass, editor, order and removal. */
export function RigEffectRow(props: RigEffectRowProps) {
  const { effect } = props;
  return (
    <div className={styles.effect} data-bypassed={effect.bypassed}>
      <span className={styles.effectName} title={effect.name}>
        {effect.name}
      </span>
      <button
        type="button"
        aria-pressed={effect.bypassed}
        aria-label={`Bypass ${effect.name}`}
        onClick={props.onBypass}
      >
        Bypass
      </button>
      <button
        type="button"
        aria-label={`Open ${effect.name} editor`}
        disabled={effect.bypassed || effect.plugin.disabledPlaceholder}
        onClick={props.onOpenEditor}
      >
        <Icon name="expand" />
      </button>
      <button
        type="button"
        aria-label={`Move ${effect.name} left`}
        disabled={props.first}
        onClick={() => props.onMove(-1)}
      >
        <Icon name="chevron" className={styles.left} />
      </button>
      <button
        type="button"
        aria-label={`Move ${effect.name} right`}
        disabled={props.last}
        onClick={() => props.onMove(1)}
      >
        <Icon name="chevron" />
      </button>
      <button type="button" aria-label={`Remove ${effect.name}`} onClick={props.onRemove}>
        <Icon name="trash" />
      </button>
    </div>
  );
}
