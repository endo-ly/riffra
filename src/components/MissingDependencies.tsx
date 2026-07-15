import { useState } from 'react';
import clsx from 'clsx';
import type { MissingDependency } from '@/lib/domain';
import { Icon } from './ui';
import styles from './MissingDependencies.module.css';

interface MissingDependenciesProps {
  missing: MissingDependency[];
  onRelink: (item: MissingDependency, newPath: string) => void;
  onDisablePlugin: (deviceId: string) => void;
  onIgnore: (item: MissingDependency) => void;
}

/**
 * Surfaces the Missing List required by PRJ-004. The project has already
 * opened (it never blocks on a missing file or plugin); this panel lets the
 * user relink, replace, ignore, or keep a missing plugin as a disabled
 * placeholder.
 */
export function MissingDependencies({
  missing,
  onRelink,
  onDisablePlugin,
  onIgnore,
}: MissingDependenciesProps) {
  const [relinkTargets, setRelinkTargets] = useState<Record<string, string>>({});

  if (missing.length === 0) return null;

  return (
    <section className={styles.missingDependencies} aria-label="Missing dependencies">
      <header>
        <span className="eyebrow">MISSING DEPENDENCIES · {missing.length}</span>
        <p>
          The project opened despite missing references. Relink, replace, or ignore each one, or
          keep a missing plugin as a disabled placeholder.
        </p>
      </header>
      <ul>
        {missing.map((item) => {
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
                  className="text-button"
                  disabled={!newPath.trim()}
                  onClick={() => onRelink(item, newPath.trim())}
                >
                  <Icon name="link" /> Relink
                </button>
                {item.kind === 'plugin' && (
                  <button className="text-button" onClick={() => onDisablePlugin(item.id)}>
                    Disable placeholder
                  </button>
                )}
                <button className="text-button" onClick={() => onIgnore(item)}>
                  Ignore
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
