import type { ArrangementMutationResult, CanonicalState, SidechainSend } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { usePreviewedValue } from '@/shared/session/usePreviewedValue';
import { useCallback } from 'react';
import { SendRow } from './SendRow';

interface SidechainSendControlsProps {
  sessionId: string;
  send: SidechainSend;
  /** The other end of the sidechain: the target device from a Track, the source Track from a device. */
  endpointName: string;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onError?: (message: string) => void;
  onOpenAutomation: () => void;
}

export function SidechainSendControls({
  send,
  sessionId,
  endpointName,
  commit,
  onError,
  onOpenAutomation,
}: SidechainSendControlsProps) {
  const { api } = useSessionContext();
  const read = useCallback(
    (canonical: CanonicalState) =>
      canonical.session.arrangement.mixer.sidechainSends.find((item) => item.id === send.id)
        ?.levelDb,
    [send.id],
  );
  const level = usePreviewedValue({
    value: send.levelDb,
    sessionId,
    target: `sidechainSend:${send.id}:levelDb`,
    read,
    submit: (levelDb) => api.updateSidechainSend(send.id, { levelDb }),
    preview: (levelDb) => api.previewSidechainSendLevel(send.id, levelDb),
    disabled: !send.enabled,
    onError,
  });
  return (
    <SendRow
      label={`${endpointName} sidechain`}
      endpoint={<span title={endpointName}>{endpointName}</span>}
      level={level}
      position={send.position}
      enabled={send.enabled}
      removeLabel="Remove Sidechain"
      onPositionChange={(position) => commit(api.updateSidechainSend(send.id, { position }))}
      onEnabledChange={(enabled) => commit(api.updateSidechainSend(send.id, { enabled }))}
      onOpenAutomation={onOpenAutomation}
      onRemove={() => commit(api.removeSidechainSend(send.id))}
    />
  );
}
