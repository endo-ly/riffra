import { useState } from 'react';
import { useSession } from '@/session/session-context';
import { selectHostReady } from '@/session/selectors';
import { Notice } from '@/shared/ui/message/Notice';
import { MissingDependenciesDialog } from './MissingDependencies';

export function MissingDependenciesNotice() {
  const count = useSession((state) => state.missing.items.length);
  const hostConnected = useSession(selectHostReady);
  const [reviewing, setReviewing] = useState(false);
  if (count === 0) return null;
  return (
    <>
      <Notice
        tone="warning"
        title="Missing dependencies"
        message={
          count === 1
            ? '1 reference could not be found. The project opened without it.'
            : `${count} references could not be found. The project opened without them.`
        }
        actions={[
          { label: 'Review…', onClick: () => setReviewing(true), disabled: !hostConnected },
        ]}
      />
      {reviewing && hostConnected && (
        <MissingDependenciesDialog onClose={() => setReviewing(false)} />
      )}
    </>
  );
}
