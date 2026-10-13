import type {
  CanonicalState,
  CreativeSession,
  EffectOwner,
  ArrangementMutationResult,
  MixBusKind,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { useState } from 'react';
import { MixdownDialog } from './MixdownDialog';
import { MixerRouting } from './MixerRouting';
import { MixerBusChannelStrip } from './MixerBusChannelStrip';
import { MixerMasterChannelStrip } from './MixerMasterChannelStrip';
import { MixerTrackChannelStrip } from './MixerTrackChannelStrip';
import styles from './Mixer.module.css';

interface MixerPanelProps {
  session: CreativeSession;
  selectedTrackId: string | null;
  selectedOwner: EffectOwner | null;
  onSelectOwner: (owner: EffectOwner) => void;
  onOpenOwnerDevices: (owner: EffectOwner) => void;
  missingDeviceIds: readonly string[];
  api: ArrangeWorkspaceApi;
  applyCanonicalState: (canonical: CanonicalState) => boolean;
  onSelectTrack: (trackId: string) => void;
  onOpenDevices: (trackId: string) => void;
  onError?: (message: string) => void;
  disabled?: boolean;
}

export function MixerPanel(props: MixerPanelProps) {
  const { arrangement } = props.session;
  const [routingTrackId, setRoutingTrackId] = useState<string | null>(null);
  const [newName, setNewName] = useState('');
  const [newKind, setNewKind] = useState<MixBusKind>('group');
  const [error, setError] = useState<string | null>(null);
  const [mixdownOpen, setMixdownOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const commit = (operation: Promise<ArrangementMutationResult>) => {
    setError(null);
    void operation
      .then((result) => {
        if (props.applyCanonicalState(result.canonical) && result.projection.state === 'failed')
          setError(result.projection.message);
      })
      .catch((failure) => setError(failure instanceof Error ? failure.message : String(failure)));
  };
  const addBus = async () => {
    if (busy || !newName.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const result = await props.api.addMixBus(newName.trim(), newKind);
      if (props.applyCanonicalState(result.canonical)) {
        setNewName('');
        if (result.projection.state === 'failed') setError(result.projection.message);
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
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
            aria-label="New channel name"
            placeholder="Channel name"
            value={newName}
            onChange={(e) => setNewName(e.currentTarget.value)}
            disabled={props.disabled || busy}
          />
          <select
            aria-label="New channel kind"
            value={newKind}
            onChange={(e) => setNewKind(e.currentTarget.value as MixBusKind)}
          >
            <option value="group">Group</option>
            <option value="return">Return</option>
          </select>
          <button type="submit" disabled={props.disabled || busy || !newName.trim()}>
            Add channel
          </button>
          {selectedBus && (
            <>
              <input
                key={selectedBus.id}
                aria-label="Channel name"
                defaultValue={selectedBus.name}
                onBlur={(e) => {
                  if (e.currentTarget.value !== selectedBus.name)
                    commit(props.api.updateMixBus(selectedBus.id, { name: e.currentTarget.value }));
                }}
              />
              <button
                type="button"
                disabled={props.disabled}
                onClick={() => commit(props.api.removeMixBus(selectedBus.id))}
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
                  (lane) => lane.trackId === track.id && lane.parameter === 'volume',
                )}
                panAutomation={arrangement.automationLanes.find(
                  (lane) => lane.trackId === track.id && lane.parameter === 'pan',
                )}
                selected={props.selectedTrackId === track.id}
                missingDeviceIds={props.missingDeviceIds}
                api={props.api}
                applyCanonicalState={props.applyCanonicalState}
                onSelect={() => props.onSelectTrack(track.id)}
                onOpenDevices={() => props.onOpenDevices(track.id)}
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
                    api={props.api}
                    missingDeviceIds={props.missingDeviceIds}
                    applyCanonicalState={props.applyCanonicalState}
                    onSelect={() => props.onSelectOwner({ kind: 'bus', id: bus.id })}
                    onOpenDevices={() => props.onOpenOwnerDevices({ kind: 'bus', id: bus.id })}
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
        onOpenMixdown={() => setMixdownOpen(true)}
        onOpenDevices={() => props.onOpenOwnerDevices({ kind: 'master' })}
        missingDeviceIds={props.missingDeviceIds}
        session={props.session}
        api={props.api}
        applyCanonicalState={props.applyCanonicalState}
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
          api={props.api}
          applyCanonicalState={props.applyCanonicalState}
          onClose={() => setMixdownOpen(false)}
        />
      )}
      {routingTrack && (
        <MixerRouting
          arrangement={arrangement}
          track={routingTrack}
          api={props.api}
          commit={commit}
          onClose={() => setRoutingTrackId(null)}
        />
      )}
    </div>
  );
}
