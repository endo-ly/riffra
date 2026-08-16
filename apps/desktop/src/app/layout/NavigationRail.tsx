import { Icon } from '@/shared/ui/primitives';
import type { SidePanelView } from './SidePanel';
import styles from './NavigationRail.module.css';

interface NavigationRailProps {
  activeView: SidePanelView;
  open: boolean;
  onSelect: (view: SidePanelView) => void;
}

export function NavigationRail({ activeView, open, onSelect }: NavigationRailProps) {
  return (
    <nav className={styles.rail} aria-label="Navigation Rail" data-navigation-rail>
      <button
        type="button"
        className={activeView === 'browser' && open ? styles.active : undefined}
        aria-label="Browser"
        aria-pressed={activeView === 'browser' && open}
        title="Browser"
        onClick={() => onSelect('browser')}
      >
        <Icon name="search" />
      </button>
      <button
        type="button"
        className={activeView === 'inspector' && open ? styles.active : undefined}
        aria-label="Inspector"
        aria-pressed={activeView === 'inspector' && open}
        title="Inspector"
        onClick={() => onSelect('inspector')}
      >
        <Icon name="sliders" />
      </button>
    </nav>
  );
}
