import type {
  Arrangement,
  ArrangementMutationResult,
  DeviceSidechainCapability,
  EffectOwner,
} from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { SidechainSendControls } from '@/features/mixer/SidechainSendControls';
import controls from '@/shared/ui/controls.module.css';
import styles from './Devices.module.css';

interface DeviceSidechainEditorProps {
  arrangement: Arrangement;
  owner: EffectOwner;
  deviceId: string;
  capabilities: DeviceSidechainCapability;
  isInstrument: boolean;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onOpenAutomation: (sidechainSendId: string) => void;
}

export function DeviceSidechainEditor({
  arrangement,
  owner,
  deviceId,
  capabilities,
  isInstrument,
  commit,
  onOpenAutomation,
}: DeviceSidechainEditorProps) {
  const { api } = useSessionContext();
  const sends = arrangement.mixer.sidechainSends.filter((send) => send.targetDeviceId === deviceId);
  if (isInstrument || (!capabilities.available && sends.length === 0)) return null;
  const canAddSource = capabilities.mono || capabilities.stereo;
  const candidates = arrangement.tracks.filter(
    (track) =>
      !sends.some((send) => send.sourceTrackId === track.id) &&
      !(owner.kind === 'track' && owner.id === track.id),
  );
  const input = capabilities.stereo ? 'Stereo' : capabilities.mono ? 'Mono' : 'Unavailable';
  return (
    <section className={styles.sidechain} aria-label="Sidechain input">
      <header className={styles.subHeader}>
        <h4>
          Sidechain input · {input} · {capabilities.channels} channel
          {capabilities.channels === 1 ? '' : 's'}
        </h4>
        <select
          className={controls.actionSelect}
          aria-label="Add Sidechain source"
          value=""
          disabled={!canAddSource || !candidates.length}
          onChange={(event) => {
            if (event.currentTarget.value)
              commit(api.addSidechainSend(event.currentTarget.value, deviceId, -6, 'postFader'));
          }}
        >
          <option value="">+ Add Source</option>
          {candidates.map((track) => (
            <option key={track.id} value={track.id}>
              {track.name}
            </option>
          ))}
        </select>
      </header>
      {!canAddSource && (
        <p className={controls.hint}>
          {capabilities.reason ?? 'This VST3 does not support a mono or stereo auxiliary input.'}
        </p>
      )}
      {sends.map((send) => (
        <SidechainSendControls
          key={send.id}
          send={send}
          endpointName={
            arrangement.tracks.find((track) => track.id === send.sourceTrackId)?.name ??
            send.sourceTrackId
          }
          commit={commit}
          onOpenAutomation={() => onOpenAutomation(send.id)}
        />
      ))}
      {!candidates.length && <p className={controls.hint}>All Tracks already feed this device.</p>}
    </section>
  );
}
