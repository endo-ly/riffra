import { sidechainTargetName } from '@/model/automation';
import { resolveTrackColor } from '@/model/track-color';
import type {
  Arrangement,
  ArrangementMutationResult,
  AutomationTarget,
  Track,
} from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import controls from '@/shared/ui/controls.module.css';
import { DialogRow, DialogSection, MixerDialog } from './MixerDialog';
import { SendRow } from './SendRow';
import { SidechainSendControls } from './SidechainSendControls';

interface Props {
  arrangement: Arrangement;
  track: Track;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onClose: () => void;
  onOpenAutomation: (target: AutomationTarget) => void;
}

export function MixerRouting({ arrangement, track, commit, onClose, onOpenAutomation }: Props) {
  const { api } = useSessionContext();
  const returns = arrangement.mixer.buses.filter((bus) => bus.kind === 'return');
  const sends = arrangement.mixer.sends.filter((send) => send.trackId === track.id);
  const sidechainSends = arrangement.mixer.sidechainSends.filter(
    (send) => send.sourceTrackId === track.id,
  );
  const unusedReturns = returns.filter((bus) => !sends.some((send) => send.returnBusId === bus.id));
  return (
    <MixerDialog
      label={`${track.name} routing`}
      title={track.name}
      subtitle="Routing"
      icon="route"
      color={resolveTrackColor(track, arrangement.tracks.indexOf(track))}
      closeOnBackdrop
      onClose={onClose}
    >
      <DialogSection title="Output">
        <DialogRow label="Main output">
          <select
            className={controls.control}
            aria-label="Main output"
            value={track.outputBusId ?? ''}
            onChange={(e) => commit(api.setTrackOutput(track.id, e.currentTarget.value || null))}
          >
            <option value="">Master</option>
            {arrangement.mixer.buses
              .filter((bus) => bus.kind === 'group')
              .map((bus) => (
                <option key={bus.id} value={bus.id}>
                  {bus.name}
                </option>
              ))}
          </select>
        </DialogRow>
      </DialogSection>
      <DialogSection
        title="Sends"
        action={
          <select
            className={controls.actionSelect}
            aria-label="Add Send"
            value=""
            disabled={!unusedReturns.length}
            onChange={(e) => {
              if (e.currentTarget.value)
                commit(api.addSend(track.id, e.currentTarget.value, -6, 'postFader'));
            }}
          >
            <option value="">+ Add Send</option>
            {unusedReturns.map((bus) => (
              <option key={bus.id} value={bus.id}>
                {bus.name}
              </option>
            ))}
          </select>
        }
      >
        {sends.map((send) => {
          const returnName =
            returns.find((bus) => bus.id === send.returnBusId)?.name ?? 'Missing Return';
          return (
            <SendRow
              key={send.id}
              label={`${returnName} send`}
              endpoint={
                <select
                  className={controls.control}
                  aria-label="Return"
                  value={send.returnBusId}
                  onChange={(e) =>
                    commit(api.updateSend(send.id, { returnBusId: e.currentTarget.value }))
                  }
                >
                  {returns
                    .filter(
                      (bus) =>
                        bus.id === send.returnBusId ||
                        !sends.some((other) => other.returnBusId === bus.id),
                    )
                    .map((bus) => (
                      <option key={bus.id} value={bus.id}>
                        {bus.name}
                      </option>
                    ))}
                </select>
              }
              levelDb={send.levelDb}
              position={send.position}
              enabled={send.enabled}
              removeLabel="Remove Send"
              previewLevel={(value) => api.previewSendLevel(send.id, value)}
              submitLevel={(value) => api.updateSend(send.id, { levelDb: value })}
              commit={commit}
              onPositionChange={(position) => commit(api.updateSend(send.id, { position }))}
              onEnabledChange={(enabled) => commit(api.updateSend(send.id, { enabled }))}
              onOpenAutomation={() => onOpenAutomation({ kind: 'sendLevel', sendId: send.id })}
              onRemove={() => commit(api.removeSend(send.id))}
            />
          );
        })}
        {!sends.length && (
          <p className={controls.placeholder}>
            {returns.length
              ? 'No Sends. Add one to feed a Return channel in parallel.'
              : 'Add a Return channel to create Sends.'}
          </p>
        )}
      </DialogSection>
      <DialogSection title="Sidechain">
        {sidechainSends.map((send) => (
          <SidechainSendControls
            key={send.id}
            send={send}
            endpointName={sidechainTargetName(arrangement, send.targetDeviceId) ?? 'Unknown device'}
            commit={commit}
            onOpenAutomation={() =>
              onOpenAutomation({ kind: 'sidechainSendLevel', sidechainSendId: send.id })
            }
          />
        ))}
        {!sidechainSends.length && (
          <p className={controls.placeholder}>
            Add sidechain sends from the target device in Devices.
          </p>
        )}
      </DialogSection>
    </MixerDialog>
  );
}
