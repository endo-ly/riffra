import { useState } from 'react';
import { openScoreYaml } from '@/native/dialog';
import { notify } from '@/shared/notify';
import { useSession } from '@/session/session-context';
import { selectHostReady } from '@/session/selectors';
import { Notice } from '@/shared/ui/message/Notice';
import styles from '@/features/project/ScoreSyncNotice.module.css';

const MAX_DIAGNOSTICS = 5;

export function ScoreSyncNotice() {
  const scoreSync = useSession((state) => state.project.scoreSync);
  const projectSwitching = useSession((state) => state.project.switching);
  const rebuildScore = useSession((state) => state.project.rebuildScore);
  const hostConnected = useSession(selectHostReady);
  const [rebuilding, setRebuilding] = useState(false);
  if (scoreSync?.state !== 'failed') return null;

  const rebuild = async (options?: { path: string }) => {
    setRebuilding(true);
    try {
      await rebuildScore(options);
    } finally {
      setRebuilding(false);
    }
  };
  const relink = async () => {
    const path = await openScoreYaml().catch((error: unknown) => {
      notify.error('Score could not be selected', { cause: error });
      return null;
    });
    if (path) await rebuild({ path });
  };
  const disabled = !hostConnected || projectSwitching || rebuilding;
  const diagnostics = scoreSync.diagnostics ?? [];
  return (
    <Notice
      tone="danger"
      title="Score sync failed"
      message={`${scoreSync.path ? `${scoreSync.path} could not be applied. ` : ''}The Project keeps its last successful sync.`}
      actions={[
        { label: 'Retry', onClick: () => void rebuild(), disabled },
        { label: 'Link another Score…', onClick: () => void relink(), disabled },
      ]}
      details={
        diagnostics.length > 0
          ? {
              summary: `${diagnostics.length} issue${diagnostics.length === 1 ? '' : 's'}`,
              content: (
                <ul className={styles.diagnostics}>
                  {diagnostics.slice(0, MAX_DIAGNOSTICS).map((diagnostic, index) => (
                    <li key={`${diagnostic.path}-${index}`}>
                      <code>
                        {diagnostic.path || 'score'}
                        {diagnostic.line > 0 ? `:${diagnostic.line}:${diagnostic.column}` : ''}
                      </code>{' '}
                      {diagnostic.message}
                    </li>
                  ))}
                </ul>
              ),
            }
          : undefined
      }
    />
  );
}
