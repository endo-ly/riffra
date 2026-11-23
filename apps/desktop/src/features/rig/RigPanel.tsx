import { useState } from 'react';
import type { Rig } from '@/model/generated';
import { useSession } from '@/session/session-context';
import { ContextMenu } from '@/shared/ui/ContextMenu';
import { Icon } from '@/shared/ui/Icon';
import controls from '@/shared/ui/controls.module.css';
import { nextRigName } from './model/rig';
import { RigDetail } from './RigDetail';
import { RigSwitcher } from './RigSwitcher';
import styles from './RigPanel.module.css';

interface RigPanelProps {
  /** The rig the panel shows, or `null` while there is none. */
  selected: Rig | null;
  onSelect: (rigId: string) => void;
  onPickEffect: (rigId: string) => void;
  onDelete: (rig: Rig) => void;
}

/** The chosen rig's playback, name and actions above its signal path. */
export function RigPanel({ selected, onSelect, onPickEffect, onDelete }: RigPanelProps) {
  const rigStore = useSession((state) => state.rig);
  const inputs = useSession((state) => state.audio.status.inputChannels);
  const [renaming, setRenaming] = useState(false);
  const [menuAnchor, setMenuAnchor] = useState<{ x: number; y: number } | null>(null);
  const { rigs, activeRigId } = rigStore.state;

  const create = async () => {
    const rigId = await rigStore.create(nextRigName(rigs), inputs[0]?.index ?? 0);
    if (rigId) onSelect(rigId);
  };

  if (!selected) {
    return (
      <div className={styles.panel} role="dialog" aria-label="Rigs">
        <div className={styles.empty}>
          <p className={controls.placeholder}>No rigs yet. A rig keeps an input and its effects.</p>
          <button type="button" className={controls.primaryButton} onClick={() => void create()}>
            <Icon name="plus" /> New Rig
          </button>
        </div>
      </div>
    );
  }

  const playing = selected.id === activeRigId;
  const rename = async (typed: string | null) => {
    setRenaming(false);
    const name = typed?.trim();
    if (name && name !== selected.name) await rigStore.update(selected.id, { name });
  };
  // While a rig plays, switching to another plays that one instead.
  const switchTo = (rigId: string) => {
    onSelect(rigId);
    if (activeRigId && rigId !== activeRigId) void rigStore.start(rigId);
  };
  const duplicate = async () => {
    const copyId = await rigStore.duplicate(selected.id);
    if (copyId) onSelect(copyId);
  };

  return (
    <div className={styles.panel} role="dialog" aria-label="Rigs">
      <header className={styles.header}>
        <button
          type="button"
          className={styles.play}
          aria-pressed={playing}
          aria-label={`${playing ? 'Stop' : 'Play'} ${selected.name}`}
          title={playing ? 'Stop' : 'Play'}
          onClick={() => void (playing ? rigStore.stop() : rigStore.start(selected.id))}
        >
          <Icon name={playing ? 'stop' : 'play'} />
        </button>
        <RigSwitcher
          rigs={rigs}
          selected={selected}
          activeRigId={activeRigId}
          renaming={renaming}
          onSelect={switchTo}
          onRename={(rigId) => {
            onSelect(rigId);
            setRenaming(true);
          }}
          onRenameEnd={(name) => void rename(name)}
          onDelete={onDelete}
        />
        <button
          type="button"
          className={controls.iconButton}
          aria-label="New rig"
          title="New rig"
          onClick={() => void create()}
        >
          <Icon name="plus" />
        </button>
        <button
          type="button"
          className={controls.iconButton}
          aria-label="Rig actions"
          aria-haspopup="menu"
          aria-expanded={menuAnchor !== null}
          onMouseDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            const bounds = event.currentTarget.getBoundingClientRect();
            setMenuAnchor((current) => (current ? null : { x: bounds.left, y: bounds.bottom + 4 }));
          }}
        >
          <Icon name="more" />
        </button>
        {menuAnchor && (
          <ContextMenu
            x={menuAnchor.x}
            y={menuAnchor.y}
            items={[
              { label: 'Rename', onClick: () => setRenaming(true) },
              { label: 'Duplicate', onClick: () => void duplicate() },
              { separator: true },
              { label: 'Delete', danger: true, onClick: () => onDelete(selected) },
            ]}
            onClose={() => setMenuAnchor(null)}
          />
        )}
      </header>
      <RigDetail
        key={selected.id}
        rig={selected}
        playing={playing}
        onPickEffect={() => onPickEffect(selected.id)}
      />
    </div>
  );
}
