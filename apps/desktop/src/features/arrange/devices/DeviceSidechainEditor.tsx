import type {
  Arrangement,
  ArrangementMutationResult,
  DeviceSidechainCapability,
  EffectOwner,
} from '@/model/domain';
import type { ArrangeWorkspaceApi } from '../arrange-api';
import { SidechainSendControls } from '@/features/mixer/SidechainSendControls';
import styles from './Devices.module.css';

interface DeviceSidechainEditorProps {
  arrangement: Arrangement;
  owner: EffectOwner;
  deviceId: string;
  capabilities: DeviceSidechainCapability;
  isInstrument: boolean;
  api: ArrangeWorkspaceApi;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onOpenAutomation: (sidechainSendId: string) => void;
}

export function DeviceSidechainEditor({
  arrangement,
  owner,
  deviceId,
  capabilities,
  isInstrument,
  api,
  commit,
  onOpenAutomation,
}: DeviceSidechainEditorProps) {
  if (!capabilities.available || isInstrument) return null;
  const sends = arrangement.mixer.sidechainSends.filter((send) => send.targetDeviceId === deviceId);
  const canAddSource = capabilities.mono || capabilities.stereo;
  const candidates = arrangement.tracks.filter(
    (track) =>
      !sends.some((send) => send.sourceTrackId === track.id) &&
      !(owner.kind === 'track' && owner.id === track.id),
  );
  const input = capabilities.stereo ? 'Stereo' : capabilities.mono ? 'Mono' : 'Unavailable';
  return (
    <section className={styles.sidechain} aria-label="Sidechain input">
      <h4>
        Sidechain input · {input} · {capabilities.channels} channel
        {capabilities.channels === 1 ? '' : 's'}
      </h4>
      {sends.map((send) => (
        <SidechainSendControls
          key={send.id}
          send={send}
          title={
            arrangement.tracks.find((track) => track.id === send.sourceTrackId)?.name ??
            send.sourceTrackId
          }
          api={api}
          commit={commit}
          onOpenAutomation={() => onOpenAutomation(send.id)}
        />
      ))}
      <label>
        Add Source{' '}
        <select
          aria-label="Add Sidechain source"
          value=""
          disabled={!canAddSource || !candidates.length}
          onChange={(event) => {
            if (event.currentTarget.value)
              commit(api.addSidechainSend(event.currentTarget.value, deviceId, -6, 'postFader'));
          }}
        >
          <option value="">Choose Track</option>
          {candidates.map((track) => (
            <option key={track.id} value={track.id}>
              {track.name}
            </option>
          ))}
        </select>
      </label>
      {!candidates.length && <p>All Tracks already feed this device.</p>}
    </section>
  );
}
