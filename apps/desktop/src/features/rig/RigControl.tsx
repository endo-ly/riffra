import { useEffect, useRef, useState } from 'react';
import { useSession } from '@/session/session-context';
import { PluginPicker } from '@/features/devices/PluginPicker';
import type { Rig } from '@/model/generated';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import { RigBar } from './RigBar';
import { RigPanel } from './RigPanel';
import styles from './RigControl.module.css';

/** The rig section of the global bar and the panel behind its `▾`. */
export function RigControl() {
  const rig = useSession((state) => state.rig);
  const plugins = useSession((state) => state.catalog.plugins);
  const [open, setOpen] = useState(false);
  const [newName, setNewName] = useState('');
  const [pickerRigId, setPickerRigId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Rig | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const active = rig.state.rigs.find((item) => item.id === rig.state.activeRigId) ?? null;

  useEffect(() => {
    if (!open) return;
    // A picker or confirmation renders outside this root and handles its own dismissal.
    const overlayOpen = pickerRigId !== null || pendingDelete !== null;
    const onPointerDown = (event: MouseEvent) => {
      if (overlayOpen || rootRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !overlayOpen) setOpen(false);
    };
    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [open, pickerRigId, pendingDelete]);

  const start = (rigId: string) => {
    void rig.start(rigId).then((started) => {
      if (!started) setOpen(true);
    });
  };
  const stop = () => {
    void rig.stop().then((stopped) => {
      if (!stopped) setOpen(true);
    });
  };
  const openFirstEditor = () => {
    const device = active?.effects.find(
      (effect) => !effect.bypassed && !effect.plugin.disabledPlaceholder,
    );
    if (active && device) void rig.openEditor(active.id, device.id);
  };

  return (
    <div className={styles.rig} ref={rootRef}>
      <RigBar
        active={active}
        open={open}
        onToggle={() => setOpen((value) => !value)}
        onStop={stop}
        onOpenEditor={openFirstEditor}
      />
      {open && (
        <RigPanel
          newName={newName}
          onNewNameChange={setNewName}
          onStart={start}
          onStop={stop}
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
