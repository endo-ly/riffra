import type { ArrangementMutationResult, CanonicalState, Send } from '@/model/generated';
import { useSessionContext } from '@/shared/session/session-context';
import { usePreviewedValue } from '@/shared/session/usePreviewedValue';
import { useCallback, type ReactNode } from 'react';
import { SendRow } from './SendRow';

interface ReturnSendControlsProps {
  sessionId: string;
  send: Send;
  label: string;
  endpoint: ReactNode;
  commit: (operation: Promise<ArrangementMutationResult>) => void;
  onError: (message: string) => void;
  onPositionChange: (position: Send['position']) => void;
  onOpenAutomation: () => void;
  onRemove: () => void;
}

/** Owns the editable level for one Track-to-Return send. */
export function ReturnSendControls(props: ReturnSendControlsProps) {
  const { api } = useSessionContext();
  const read = useCallback(
    (canonical: CanonicalState) =>
      canonical.session.arrangement.mixer.sends.find((send) => send.id === props.send.id)?.levelDb,
    [props.send.id],
  );
  const level = usePreviewedValue({
    value: props.send.levelDb,
    sessionId: props.sessionId,
    target: `send:${props.send.id}:levelDb`,
    read,
    submit: (levelDb) => api.updateSend(props.send.id, { levelDb }),
    preview: (levelDb) => api.previewSendLevel(props.send.id, levelDb),
    disabled: !props.send.enabled,
    onError: props.onError,
  });

  return (
    <SendRow
      label={props.label}
      endpoint={props.endpoint}
      level={level}
      position={props.send.position}
      enabled={props.send.enabled}
      removeLabel="Remove Send"
      onPositionChange={props.onPositionChange}
      onEnabledChange={(enabled) => props.commit(api.updateSend(props.send.id, { enabled }))}
      onOpenAutomation={props.onOpenAutomation}
      onRemove={props.onRemove}
    />
  );
}
