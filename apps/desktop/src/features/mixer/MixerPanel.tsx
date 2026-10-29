import type {
  ArrangementMutationResult,
  AutomationTarget,
  CreativeSession,
  EffectOwner,
  MixBusKind,
} from '@/model/domain';
import { useSessionContext } from '@/shared/session/session-context';
import controls from '@/shared/ui/controls.module.css';
import { useState } from 'react';
import { MixdownDialog } from './MixdownDialog';
import styles from './Mixer.module.css';
import { MixerBusChannelStrip } from './MixerBusChannelStrip';
import { MixerMasterChannelStrip } from './MixerMasterChannelStrip';
import { MixerRouting } from './MixerRouting';
import { MixerTrackChannelStrip } from './MixerTrackChannelStrip';

interface MixerPanelProps {
  session: CreativeSession;
  selectedTrackId: string | null;
  selectedOwner: EffectOwner | null;
  onSelectOwner: (owner: EffectOwner) => void;
  onOpenOwnerDevices: (owner: EffectOwner) => void;
  missingDeviceIds: readonly string[];
  onSelectTrack: (trackId: string) => void;
  onOpenDevices: (trackId: string) => void;
  onOpenAutomation: (target: AutomationTarget) => void;
  onError?: (message: string) => void;
  disabled?: boolean;
}

export function MixerPanel(props: MixerPanelProps) {
  const { api, commit: sessionCommit } = useSessionContext();
  const { arrangement } = props.session;
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
    props.selectedOwner?.kind === 'bus'
      ? arrangement.mixer.buses.find(
          (bus) => props.selectedOwner?.kind === 'bus' && bus.id === props.selectedOwner.id,
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
            disabled={props.disabled || busy}
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
            disabled={props.disabled || busy || !newName.trim()}
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
                disabled={props.disabled}
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
                sessionId={props.session.sessionId}
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
                selected={props.selectedTrackId === track.id}
                missingDeviceIds={props.missingDeviceIds}
                onSelect={() => props.onSelectTrack(track.id)}
                onOpenDevices={() => props.onOpenDevices(track.id)}
                onOpenAutomation={() =>
                  props.onOpenAutomation({
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
                onError={props.onError}
                disabled={props.disabled}
              />
            ))}
            {(['group', 'return'] as const).flatMap((kind) =>
              arrangement.mixer.buses
                .filter((bus) => bus.kind === kind)
                .map((bus) => (
                  <MixerBusChannelStrip
                    key={bus.id}
                    sessionId={props.session.sessionId}
                    bus={bus}
                    selected={
                      props.selectedOwner?.kind === 'bus' && props.selectedOwner.id === bus.id
                    }
                    missingDeviceIds={props.missingDeviceIds}
                    onSelect={() => props.onSelectOwner({ kind: 'bus', id: bus.id })}
                    onOpenDevices={() => props.onOpenOwnerDevices({ kind: 'bus', id: bus.id })}
                    onOpenAutomation={() =>
                      props.onOpenAutomation({ kind: 'busMix', busId: bus.id, parameter: 'gain' })
                    }
                    automated={arrangement.automationLanes.some(
                      (lane) => lane.target.kind === 'busMix' && lane.target.busId === bus.id,
                    )}
                    onError={setError}
                    disabled={props.disabled}
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
        onOpenAutomation={() => props.onOpenAutomation({ kind: 'masterGain' })}
        onOpenMixdown={() => setMixdownOpen(true)}
        onOpenDevices={() => props.onOpenOwnerDevices({ kind: 'master' })}
        missingDeviceIds={props.missingDeviceIds}
        session={props.session}
        disabled={props.disabled}
      />
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {mixdownOpen && (
        <MixdownDialog
          key={props.session.sessionId}
          session={props.session}
          onClose={() => setMixdownOpen(false)}
        />
      )}
      {routingTrack && (
        <MixerRouting
          onOpenAutomation={props.onOpenAutomation}
          arrangement={arrangement}
          track={routingTrack}
          commit={commit}
          onClose={() => setRoutingTrackId(null)}
        />
      )}
    </div>
  );
}
