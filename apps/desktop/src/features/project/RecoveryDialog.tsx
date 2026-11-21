import type { RecoveryCandidate } from '@/model/generated';
import { useSession } from '@/session/session-context';
import { Dialog } from '@/shared/ui/Dialog';
import controls from '@/shared/ui/controls.module.css';
import styles from '@/features/project/RecoveryDialog.module.css';

const MAX_CANDIDATES = 5;

const updatedFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

/** Lists the stable autosave generations the recovered session can be replaced with. */
export function RecoveryDialog({
  candidates,
  onClose,
}: {
  candidates: readonly RecoveryCandidate[];
  onClose: () => void;
}) {
  const restoreRecovery = useSession((state) => state.project.restoreRecovery);
  const projectSwitching = useSession((state) => state.project.switching);
  const restore = (fileName: string) => {
    onClose();
    void restoreRecovery(fileName);
  };
  return (
    <Dialog
      label="Choose a stable generation"
      title="Choose a stable generation"
      subtitle="The current session is replaced by the generation you restore."
      icon="restore"
      closeOnBackdrop
      onClose={onClose}
    >
      <ul className={styles.candidates}>
        {candidates.slice(0, MAX_CANDIDATES).map((candidate) => (
          <li key={candidate.fileName} className={styles.candidate}>
            <span className={styles.identity}>
              <strong>{candidate.projectName ?? 'Untitled'}</strong>
              <small>{updatedFormat.format(new Date(candidate.updatedAtMs))}</small>
            </span>
            <button
              type="button"
              className={controls.smallButton}
              disabled={projectSwitching}
              onClick={() => restore(candidate.fileName)}
            >
              Restore
            </button>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
