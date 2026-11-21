import { useState } from 'react';
import { useSession } from '@/session/session-context';
import clsx from 'clsx';
import type { MissingDependency } from '@/model/generated';
import { Dialog } from '@/shared/ui/Dialog';
import { Icon } from '@/shared/ui/Icon';
import surface from '@/shared/ui/Surface.module.css';
import styles from './MissingDependencies.module.css';

export function MissingDependenciesDialog({ onClose }: { onClose: () => void }) {
  const missing = useSession((state) => state.missing);
  const items = missing.items;
  const onRelink = (item: MissingDependency, newPath: string) => void missing.relink(item, newPath);
  const onReplacePlugin = (deviceId: string, newPath: string) =>
    void missing.replacePlugin(deviceId, newPath);
  const onDisablePlugin = (deviceId: string) => void missing.disablePlugin(deviceId);
  const onIgnore = (item: MissingDependency) => missing.ignore(item);

  const [relinkTargets, setRelinkTargets] = useState<Record<string, string>>({});

  if (items.length === 0) return null;

  return (
    <Dialog
      label="Missing dependencies"
      title="Missing dependencies"
      subtitle="Relink, replace, or ignore each one, or keep a missing plugin as a disabled placeholder."
      icon="link"
      onClose={onClose}
    >
      <section className={styles.missingDependencies} aria-label="Missing dependencies">
        <ul>
          {items.map((item) => {
            const key = `${item.kind}:${item.id}`;
            const newPath = relinkTargets[key] ?? '';
            return (
              <li key={key} className={clsx(styles.missingItem, `missing-${item.kind}`)}>
                <div className={styles.missingDetail}>
                  <strong>{item.name}</strong>
                  <small className={styles.missingKind}>{item.kind}</small>
                  <small className={styles.missingPath}>{item.path || 'no path stored'}</small>
                  <small className={styles.missingUsedBy}>used by: {item.usedBy.join(', ')}</small>
                </div>
                <div className={styles.missingActions}>
                  <input
                    aria-label={`Relink path for ${item.name}`}
                    placeholder="Replacement path"
                    value={newPath}
                    onChange={(event) =>
                      setRelinkTargets((current) => ({ ...current, [key]: event.target.value }))
                    }
                  />
                  <button
                    className={surface.textButton}
                    disabled={!newPath.trim()}
                    onClick={() =>
                      item.kind === 'plugin'
                        ? onReplacePlugin(item.id, newPath.trim())
                        : onRelink(item, newPath.trim())
                    }
                  >
                    <Icon name="link" /> {item.kind === 'plugin' ? 'Replace' : 'Relink'}
                  </button>
                  {item.kind === 'plugin' && (
                    <button className={surface.textButton} onClick={() => onDisablePlugin(item.id)}>
                      Disable placeholder
                    </button>
                  )}
                  <button className={surface.textButton} onClick={() => onIgnore(item)}>
                    Ignore
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      </section>
    </Dialog>
  );
}
