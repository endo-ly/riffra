import { useSessionCommand } from '@/session/hooks/useSessionCommand';
import type { ArrangementMutationResult, EffectOwner, MixBusKind } from '@/model/generated';
import { useSession, useSessionContext } from '@/session/session-context';
import { useShallow } from 'zustand/react/shallow';
import { selectMissingDeviceIds } from '@/session/selectors';
import { ownerSelection, selectionOwner } from '@/model/editor-selection';
import controls from '@/shared/ui/controls.module.css';
import { useState } from 'react';
import { MixdownDialog } from '@/features/render/MixdownDialog';
import styles from './Mixer.module.css';
import { MixerBusChannelStrip } from './MixerBusChannelStrip';
import { MixerMasterChannelStrip } from './MixerMasterChannelStrip';
import { MixerRouting } from './MixerRouting';
import { MixerTrackChannelStrip } from './MixerTrackChannelStrip';
import { InlineMessage } from '@/shared/ui/message/InlineMessage';

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
  const [newName, setNewName] = useState('');
  const [newKind, setNewKind] = useState<MixBusKind>('group');
  const [error, setError] = useState<string | null>(null);
  const [mixdownOpen, setMixdownOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const commitMutation = sessionCommit;
  const commit = (operation: Promise<ArrangementMutationResult>) => {
    setError(null);
    void commitMutation(operation, { onError: setError });
  };
  const addBus = async () => {
    if (busy || !newName.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await commitMutation(api.addMixBus(newName.trim(), newKind), {
        onError: setError,
      });
      if (result) setNewName('');
    } finally {
      setBusy(false);
    }
  };
  const selectedBus =
    selectedOwner?.kind === 'bus'
      ? arrangement.mixer.buses.find(
          (bus) => selectedOwner?.kind === 'bus' && bus.id === selectedOwner.id,
        )
      : undefined;
  const routingTrack = arrangement.tracks.find((track) => track.id === routingTrackId);

  return (
    <div className={styles.panel} aria-label="Mixer" data-mixer-panel>
      <header className={styles.panelHeader}>
        <div>
          <strong>MIXER</strong>
          <span>Tracks, Groups, Returns and Master</span>
        </div>
        <form
          className={styles.busControls}
          onSubmit={(e) => {
            e.preventDefault();
            void addBus();
          }}
        >
          <input
            className={controls.control}
            aria-label="New channel name"
            placeholder="Channel name"
            value={newName}
            onChange={(e) => setNewName(e.currentTarget.value)}
            disabled={busy}
          />
          <select
            className={controls.control}
            aria-label="New channel kind"
            value={newKind}
            onChange={(e) => setNewKind(e.currentTarget.value as MixBusKind)}
          >
            <option value="group">Group</option>
            <option value="return">Return</option>
          </select>
          <button
            type="submit"
            className={`${controls.smallButton} ${controls.accent}`}
            disabled={busy || !newName.trim()}
          >
            Add channel
          </button>
          {selectedBus && (
            <>
              <input
                className={controls.control}
                key={selectedBus.id}
                aria-label="Channel name"
                defaultValue={selectedBus.name}
                onBlur={(e) => {
                  if (e.currentTarget.value !== selectedBus.name)
                    commit(api.updateMixBus(selectedBus.id, { name: e.currentTarget.value }));
                }}
              />
              <button
                type="button"
                className={`${controls.smallButton} ${controls.danger}`}
                onClick={() => commit(api.removeMixBus(selectedBus.id))}
              >
                Remove channel
              </button>
            </>
          )}
        </form>
      </header>

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
                    automated={arrangement.automationLanes.some(
                      (lane) => lane.target.kind === 'busMix' && lane.target.busId === bus.id,
                    )}
                    onError={setError}
                  />
                )),
            )}
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
