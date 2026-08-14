import {
  SNAP_GRID_OPTIONS,
  snapGridLabel,
  type ArrangeTool,
  type SnapGrid,
} from '@/features/arrange/model/arrange-timeline';
import { Icon } from '@/shared/ui/primitives';
import styles from '../WorkspaceArrange.module.css';

interface ArrangeToolbarProps {
  tool: ArrangeTool;
  snap: SnapGrid;
  zoom: number;
  rulerMode: 'bars' | 'time';
  follow: boolean;
  onTool: (tool: ArrangeTool) => void;
  onSnap: (snap: SnapGrid) => void;
  onZoom: (zoom: number) => void;
  onRulerMode: (mode: 'bars' | 'time') => void;
  onFollow: (follow: boolean) => void;
  automationAvailable: boolean;
  automationOpen: boolean;
  onToggleAutomation: () => void;
}

export function ArrangeToolbar(props: ArrangeToolbarProps) {
  return (
    <header className={styles.toolbar}>
      <div className={styles.segmented} aria-label="Arrange tool">
        <button
          className={props.tool === 'select' ? styles.active : ''}
          onClick={() => props.onTool('select')}
        >
          <Icon name="pointer" /> Select
        </button>
        <button
          className={props.tool === 'split' ? styles.active : ''}
          onClick={() => props.onTool('split')}
        >
          <Icon name="scissors" /> Split
        </button>
      </div>

      <label className={styles.compactField}>
        <span>SNAP</span>
        <select
          value={props.snap}
          onChange={(event) => props.onSnap(event.target.value as SnapGrid)}
        >
          {SNAP_GRID_OPTIONS.map((value) => (
            <option key={value} value={value}>
              {snapGridLabel(value)}
            </option>
          ))}
        </select>
      </label>

      <button
        className={`${styles.toggleButton} ${props.follow ? styles.active : ''}`}
        aria-pressed={props.follow}
        title="Keep the playhead in view during playback"
        onClick={() => props.onFollow(!props.follow)}
      >
        Follow
      </button>
      <button
        className={`${styles.toggleButton} ${props.automationOpen ? styles.active : ''}`}
        aria-pressed={props.automationOpen}
        disabled={!props.automationAvailable}
        title={
          props.automationAvailable
            ? 'Show or hide Automation for the selected Track'
            : 'Select a Track to edit Automation'
        }
        onClick={props.onToggleAutomation}
      >
        Automation
      </button>

      <div className={styles.toolbarRight}>
        <div className={styles.segmented} aria-label="Ruler display">
          <button
            className={props.rulerMode === 'bars' ? styles.active : ''}
            onClick={() => props.onRulerMode('bars')}
          >
            Bars
          </button>
          <button
            className={props.rulerMode === 'time' ? styles.active : ''}
            onClick={() => props.onRulerMode('time')}
          >
            Time
          </button>
        </div>
        <label className={styles.zoomField}>
          <span>−</span>
          <input
            aria-label="Timeline zoom"
            type="range"
            min="0.35"
            max="4"
            step="0.05"
            value={props.zoom}
            onChange={(event) => props.onZoom(Number(event.target.value))}
          />
          <span>＋</span>
        </label>
      </div>
    </header>
  );
}
