import { useEffect, useRef, useState } from 'react';
import { useSession } from '@/session/session-context';
import { PluginPicker } from '@/features/devices/PluginPicker';
import type { Rig } from '@/model/generated';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { RigPanel } from './RigPanel';
import { RigTrigger } from './RigTrigger';
import styles from './RigControl.module.css';

/** The rig section of the global bar and the panel it opens. */
export function RigControl() {
  const rig = useSession((state) => state.rig);
  const plugins = useSession((state) => state.catalog.plugins);
  const [open, setOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pickerRigId, setPickerRigId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Rig | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const { rigs, activeRigId, lastPlayedRigId } = rig.state;
  const find = (rigId: string | null) => rigs.find((item) => item.id === rigId);
  const active = find(activeRigId) ?? null;
  const selected = find(selectedId) ?? active ?? find(lastPlayedRigId) ?? rigs[0] ?? null;

  useEffect(() => {
    if (!open) return;
    // A picker or confirmation renders outside this root and handles its own dismissal.
    const overlayOpen = pickerRigId !== null || pendingDelete !== null;
    const onPointerDown = (event: PointerEvent) => {
      if (overlayOpen || rootRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    // A menu open inside the panel takes Escape for itself.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || overlayOpen) return;
      if (!rootRef.current?.querySelector('[role="menu"]')) setOpen(false);
    };
    // Captured, so surfaces that cancel the pointer's default still dismiss the panel.
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [open, pickerRigId, pendingDelete]);

  return (
    <div className={styles.rig} ref={rootRef}>
      <RigTrigger
        rig={active ?? selected}
        playing={active !== null}
        open={open}
        onToggle={() => setOpen((value) => !value)}
        onPlay={() => {
          if (selected) void rig.start(selected.id);
        }}
        onStop={() => void rig.stop()}
      />
      {open && (
        <RigPanel
          selected={selected}
          onSelect={setSelectedId}
          onPickEffect={setPickerRigId}
          onDelete={setPendingDelete}
        />
      )}
      {pickerRigId && (
        <PluginPicker
          plugins={plugins}
          role="effect"
          title="Add Rig Effect"
          onSelectPlugin={(plugin) => {
            void rig.addEffect(pickerRigId, plugin.path);
            setPickerRigId(null);
          }}
          onClose={() => setPickerRigId(null)}
        />
      )}
      {pendingDelete && (
        <ConfirmDialog
          title="Delete rig?"
          message={`"${pendingDelete.name}" and its effect settings will be deleted permanently.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            const rigId = pendingDelete.id;
            setPendingDelete(null);
            void rig.remove(rigId);
          }}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </div>
  );
}
