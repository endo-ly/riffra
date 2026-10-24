import type { ArrangementMutationResult, SidechainSend } from '@/model/domain';
import type { ArrangeWorkspaceApi } from '@/features/arrange/arrange-api';
import { SendRow } from './SendRow';

interface SidechainSendControlsProps {
  send: SidechainSend;
  /** The other end of the sidechain: the target device from a Track, the source Track from a device. */
  endpointName: string;
  api: ArrangeWorkspaceApi;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onOpenAutomation: () => void;
}

export function SidechainSendControls({
  send,
  endpointName,
  api,
  commit,
  onOpenAutomation,
}: SidechainSendControlsProps) {
  return (
    <SendRow
      label={`${endpointName} sidechain`}
      endpoint={<span title={endpointName}>{endpointName}</span>}
      levelDb={send.levelDb}
      position={send.position}
      enabled={send.enabled}
      removeLabel="Remove Sidechain"
      previewLevel={(value) => api.previewSidechainSendLevel(send.id, value)}
      submitLevel={(value) => api.updateSidechainSend(send.id, { levelDb: value })}
      commit={commit}
      onPositionChange={(position) => commit(api.updateSidechainSend(send.id, { position }))}
      onEnabledChange={(enabled) => commit(api.updateSidechainSend(send.id, { enabled }))}
      onOpenAutomation={onOpenAutomation}
      onRemove={() => commit(api.removeSidechainSend(send.id))}
    />
  );
}
