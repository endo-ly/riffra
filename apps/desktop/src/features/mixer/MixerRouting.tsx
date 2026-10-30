import { sidechainTargetName } from '@/model/automation';
import { resolveTrackColor } from '@/model/track-color';
import type {
  Arrangement,
  ArrangementMutationResult,
  AutomationTarget,
  Track,
} from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { toast } from '@/shared/toasts';
import controls from '@/shared/ui/controls.module.css';
import { Dialog, DialogRow, DialogSection } from '@/shared/ui/Dialog';
import { ReturnSendControls } from './ReturnSendControls';
import { SidechainSendControls } from './SidechainSendControls';

interface Props {
  sessionId: string;
  arrangement: Arrangement;
  track: Track;
  onClose: () => void;
  onOpenAutomation: (target: AutomationTarget) => void;
}

export function MixerRouting({ sessionId, arrangement, track, onClose, onOpenAutomation }: Props) {
  const { api, commit: sessionCommit } = useSessionContext();
  const onError = (message: string) => toast(message, { kind: 'error' });
  const commit = (operation: Promise<ArrangementMutationResult>) => {
    void sessionCommit(operation, { onError });
  };
  const returns = arrangement.mixer.buses.filter((bus) => bus.kind === 'return');
  const sends = arrangement.mixer.sends.filter((send) => send.trackId === track.id);
  const sidechainSends = arrangement.mixer.sidechainSends.filter(
    (send) => send.sourceTrackId === track.id,
  );
  const unusedReturns = returns.filter((bus) => !sends.some((send) => send.returnBusId === bus.id));
  return (
    <Dialog
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
          const endpoint = (
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
          );
          return (
            <ReturnSendControls
              key={send.id}
              sessionId={sessionId}
              send={send}
              label={`${returnName} send`}
              endpoint={endpoint}
              commit={commit}
              onError={onError}
              onPositionChange={(position) => commit(api.updateSend(send.id, { position }))}
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
            sessionId={sessionId}
            send={send}
            endpointName={sidechainTargetName(arrangement, send.targetDeviceId) ?? 'Unknown device'}
            commit={commit}
            onError={onError}
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
    </Dialog>
  );
}
