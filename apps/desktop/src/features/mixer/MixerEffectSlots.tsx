import type { EffectDevice, EffectOwner, Track } from '@/model/generated';
import { useSessionContext } from '@/session/session-context';
import { canOpenInstrumentEditor } from '@/features/devices/model/device-target';
import { HostConnectionChangedError } from '@/native/invoke';
import { errorText } from '@/shared/errors';
import { Icon } from '@/shared/ui/Icon';
import styles from './Mixer.module.css';

interface MixerEffectSlotsProps {
  owner: EffectOwner;
  effects: EffectDevice[];
  missingDeviceIds: readonly string[];
  onOpenDevices: () => void;
  onError?: (message: string) => void;
  track?: Track;
}

/** Displays a channel's instrument and effect chain with 1-click plug-in UI launch. */
export function MixerEffectSlots({
  owner,
  effects,
  missingDeviceIds,
  onOpenDevices,
  onError,
  track,
}: MixerEffectSlotsProps) {
  const { api } = useSessionContext();
  const instrument = track?.instrument;
  const instrumentEditorReady = track ? canOpenInstrumentEditor(track, missingDeviceIds) : false;

  const openEditor = (deviceId: string) => {
    void api.openPluginEditor(owner, deviceId).catch((failure: unknown) => {
      if (failure instanceof HostConnectionChangedError) return;
      onError?.(errorText(failure));
    });
  };

  return (
    <div className={styles.deviceSlots} role="group" aria-label="Devices">
      {instrument && (
        <button
          type="button"
          className={styles.instrumentSlot}
          data-editor={instrumentEditorReady || undefined}
          data-bypassed={instrument.bypassed || undefined}
          onClick={(event) => {
            event.stopPropagation();
            if (instrumentEditorReady) openEditor(instrument.id);
            else onOpenDevices();
          }}
          title={
            instrumentEditorReady
              ? `Open ${instrument.name} editor`
              : `${instrument.name} — open Devices`
          }
        >
          <Icon name="keys" />
          <span>{instrument.name}</span>
        </button>
      )}
      {effects.map((effect) => {
        const missing = effect.plugin.disabledPlaceholder || missingDeviceIds.includes(effect.id);
        const editorReady = !effect.bypassed && !missing;
        return (
          <button
            key={effect.id}
            type="button"
            className={`${styles.effectSlot}${missing ? ` ${styles.warning}` : ''}`}
            data-editor={editorReady || undefined}
            data-bypassed={effect.bypassed || undefined}
            onClick={(event) => {
              event.stopPropagation();
              if (editorReady) openEditor(effect.id);
              else onOpenDevices();
            }}
            title={
              missing
                ? `${effect.name} (missing) — open Devices`
                : effect.bypassed
                  ? `${effect.name} (bypassed) — open Devices`
                  : `Open ${effect.name} editor`
            }
          >
            <Icon name="module" />
            <span>{effect.name}</span>
          </button>
        );
      })}
      <button
        type="button"
        className={styles.addFxSlot}
        onClick={(event) => {
          event.stopPropagation();
          onOpenDevices();
        }}
        title="Open Devices"
      >
        <Icon name="plus" />
        <span>{effects.length === 0 ? 'Add FX' : 'Devices'}</span>
      </button>
    </div>
  );
}
