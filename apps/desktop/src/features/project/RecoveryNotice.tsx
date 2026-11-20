import { useState } from 'react';
import { useSession } from '@/shared/session/session-context';
import { selectHostReady } from '@/shared/session/selectors';
import { Notice } from '@/shared/ui/message/Notice';
import { RecoveryDialog } from './RecoveryDialog';

export function RecoveryNotice() {
  const recovery = useSession((state) => state.project.boot?.recovery);
  const projectSwitching = useSession((state) => state.project.switching);
  const keepRecovered = useSession((state) => state.project.keepRecovered);
  const hostConnected = useSession(selectHostReady);
  const [choosing, setChoosing] = useState(false);
  if (!recovery?.recoveredFromGeneration || recovery.recoveryCandidates.length === 0) return null;
  return (
    <>
      <Notice
        tone="info"
        title="Recovered session"
        message="This session was recovered from an autosave generation. You can switch to an earlier stable generation."
        actions={[
          {
            label: 'Choose generation…',
            onClick: () => setChoosing(true),
            disabled: !hostConnected || projectSwitching,
          },
          { label: 'Keep recovered session', onClick: keepRecovered, disabled: projectSwitching },
        ]}
      />
      {choosing && hostConnected && (
        <RecoveryDialog
          candidates={recovery.recoveryCandidates}
          onClose={() => setChoosing(false)}
        />
      )}
    </>
  );
}
