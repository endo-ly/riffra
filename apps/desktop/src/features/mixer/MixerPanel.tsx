import { useSessionCommand } from '@/session/hooks/useSessionCommand';
import type { ArrangementMutationResult, EffectOwner, MixBus, MixBusKind } from '@/model/generated';
import { useSession, useSessionContext } from '@/session/session-context';
import { useShallow } from 'zustand/react/shallow';
import { selectMissingDeviceIds } from '@/session/selectors';
import { ownerSelection, selectionOwner } from '@/model/editor-selection';
import { Icon } from '@/shared/ui/Icon';
import controls from '@/shared/ui/controls.module.css';
import { useState } from 'react';
import { MixdownDialog } from '@/features/render/MixdownDialog';
import styles from './Mixer.module.css';
import { MixerBusChannelStrip } from './MixerBusChannelStrip';
import { MixerMasterChannelStrip } from './MixerMasterChannelStrip';
import { MixerRouting } from './MixerRouting';
import { MixerTrackChannelStrip } from './MixerTrackChannelStrip';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';

function nextBusName(buses: readonly MixBus[], kind: MixBusKind) {
  const prefix = kind === 'group' ? 'Group' : 'Return';
  const existing = new Set(buses.map((bus) => bus.name));
  let index = 1;
  while (existing.has(`${prefix} ${index}`)) index += 1;
  return `${prefix} ${index}`;
}

export function MixerPanel() {
  const { api } = useSessionContext();
  const sessionCommit = useSessionCommand();
  const session = useSession((state) => state.project.boot!.canonical.session);
  const workspace = useSession((state) => state.workspace);
  const missingDeviceIds = useSession(useShallow(selectMissingDeviceIds));
  const selectedTrackId = workspace.selection.kind === 'track' ? workspace.selection.trackId : null;
  const selectedOwner = selectionOwner(workspace.selection);
  const selectOwner = (owner: EffectOwner) => workspace.setSelection(ownerSelection(owner));
  const openOwnerDevices = (owner: EffectOwner) => {
    selectOwner(owner);
    workspace.openDevices();
  };
  const selectTrack = (trackId: string) => workspace.setSelection({ kind: 'track', trackId });
  const openTrackDevices = (trackId: string) => {
    selectTrack(trackId);
    workspace.openDevices();
  };
  const { arrangement } = session;
  const [routingTrackId, setRoutingTrackId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mixdownOpen, setMixdownOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const commitMutation = sessionCommit;
  const commit = (operation: Promise<ArrangementMutationResult>) => {
    setError(null);
    void commitMutation(operation, { onError: setError });
  };
  const addBus = async (kind: MixBusKind) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const name = nextBusName(arrangement.mixer.buses, kind);
      await commitMutation(api.addMixBus(name, kind), { onError: setError });
    } finally {
      setBusy(false);
    }
  };
  const routingTrack = arrangement.tracks.find((track) => track.id === routingTrackId);

  return (
    <div className={styles.panel} aria-label="Mixer" data-mixer-panel>
      <div className={styles.trackViewport}>
        {arrangement.tracks.length || arrangement.mixer.buses.length ? (
          <div className={styles.trackRail}>
            {arrangement.tracks.map((track, trackIndex) => (
              <MixerTrackChannelStrip
                key={track.id}
                sessionId={session.sessionId}
                track={track}
                trackIndex={trackIndex}
                volumeAutomation={arrangement.automationLanes.find(
                  (lane) =>
                    lane.target.kind === 'trackMix' &&
                    lane.target.trackId === track.id &&
                    lane.target.parameter === 'volume',
                )}
                panAutomation={arrangement.automationLanes.find(
                  (lane) =>
                    lane.target.kind === 'trackMix' &&
                    lane.target.trackId === track.id &&
                    lane.target.parameter === 'pan',
                )}
                selected={selectedTrackId === track.id}
                missingDeviceIds={missingDeviceIds}
                onSelect={() => selectTrack(track.id)}
                onOpenDevices={() => openTrackDevices(track.id)}
                onOpenAutomation={() =>
                  workspace.openAutomation({
                    kind: 'trackMix',
                    trackId: track.id,
                    parameter: 'volume',
                  })
                }
                onOpenRouting={() => setRoutingTrackId(track.id)}
                outputName={
                  arrangement.mixer.buses.find((bus) => bus.id === track.outputBusId)?.name ??
                  'Master'
                }
                sendCount={
                  arrangement.mixer.sends.filter((send) => send.trackId === track.id).length
                }
                onError={setError}
              />
            ))}
            {(['group', 'return'] as const).flatMap((kind) =>
              arrangement.mixer.buses
                .filter((bus) => bus.kind === kind)
                .map((bus) => (
                  <MixerBusChannelStrip
                    key={bus.id}
                    sessionId={session.sessionId}
                    bus={bus}
                    selected={selectedOwner?.kind === 'bus' && selectedOwner.id === bus.id}
                    missingDeviceIds={missingDeviceIds}
                    onSelect={() => selectOwner({ kind: 'bus', id: bus.id })}
                    onOpenDevices={() => openOwnerDevices({ kind: 'bus', id: bus.id })}
                    onOpenAutomation={() =>
                      workspace.openAutomation({ kind: 'busMix', busId: bus.id, parameter: 'gain' })
                    }
                    onRemove={() => commit(api.removeMixBus(bus.id))}
                    automated={arrangement.automationLanes.some(
                      (lane) => lane.target.kind === 'busMix' && lane.target.busId === bus.id,
                    )}
                    onError={setError}
                  />
                )),
            )}
            <div className={styles.addBusColumn}>
              <button
                type="button"
                className={controls.smallButton}
                disabled={busy}
                onClick={() => void addBus('group')}
              >
                <Icon name="plus" /> Group
              </button>
              <button
                type="button"
                className={controls.smallButton}
                disabled={busy}
                onClick={() => void addBus('return')}
              >
                <Icon name="plus" /> Return
              </button>
            </div>
          </div>
        ) : (
          <div className={styles.empty}>
            <strong>No tracks</strong>
            <span>Add tracks from the Timeline to start mixing.</span>
          </div>
        )}
      </div>

      <MixerMasterChannelStrip
        onOpenAutomation={() => workspace.openAutomation({ kind: 'masterGain' })}
        onOpenMixdown={() => setMixdownOpen(true)}
        onOpenDevices={() => openOwnerDevices({ kind: 'master' })}
        onError={setError}
        missingDeviceIds={missingDeviceIds}
        session={session}
      />
      {error && <InlineMessage tone="danger">{error}</InlineMessage>}
      {mixdownOpen && (
        <MixdownDialog
          key={session.sessionId}
          session={session}
          onClose={() => setMixdownOpen(false)}
        />
      )}
      {routingTrack && (
        <MixerRouting
          sessionId={session.sessionId}
          onOpenAutomation={workspace.openAutomation}
          arrangement={arrangement}
          track={routingTrack}
          onClose={() => setRoutingTrackId(null)}
        />
      )}
    </div>
  );
}
