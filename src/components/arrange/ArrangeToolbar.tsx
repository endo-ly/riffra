import { Icon } from '../shared/ui';
import type { ArrangeTool, SnapGrid, TrackSize } from '@/lib/arrange-timeline';
import styles from './WorkspaceArrange.module.css';

interface ArrangeToolbarProps {
  tool: ArrangeTool;
  snap: SnapGrid;
  zoom: number;
  trackSize: TrackSize;
  rulerMode: 'bars' | 'time';
  follow: boolean;
  position: string;
  clock: string;
  bpm: number;
  signature: string;
  onTool: (tool: ArrangeTool) => void;
  onSnap: (snap: SnapGrid) => void;
  onZoom: (zoom: number) => void;
  onTrackSize: (size: TrackSize) => void;
  onRulerMode: (mode: 'bars' | 'time') => void;
  onFollow: (follow: boolean) => void;
  onAddTrack: () => void;
}

export function ArrangeToolbar(props: ArrangeToolbarProps) {
  return (
    <header className={styles.toolbar}>
      <div className={styles.segmented} aria-label="Arrange tool">
        <button
          className={props.tool === 'select' ? styles.active : ''}
          onClick={() => props.onTool('select')}
        >
          <span className={styles.pointerGlyph}>↖</span> Select
        </button>
        <button
          className={props.tool === 'split' ? styles.active : ''}
          onClick={() => props.onTool('split')}
        >
          <span className={styles.splitGlyph}>╱</span> Split
        </button>
      </div>

      <label className={styles.compactField}>
        <span>SNAP</span>
        <select
          value={props.snap}
          onChange={(event) => props.onSnap(event.target.value as SnapGrid)}
        >
          {['bar', '1/2', '1/4', '1/8', '1/16', '1/32', '1/8t', '1/16t', 'off'].map((value) => (
            <option key={value} value={value}>
              {value === 'bar' ? '1 Bar' : value}
            </option>
          ))}
        </select>
      </label>

      <div className={styles.positionDisplay}>
        <span>POSITION</span>
        <strong>{props.position}</strong>
        <small>{props.clock}</small>
      </div>

      <div className={styles.projectTime}>
        <div>
          <span>TEMPO</span>
          <strong>{props.bpm.toFixed(1)}</strong>
        </div>
        <div>
          <span>METER</span>
          <strong>{props.signature}</strong>
        </div>
      </div>

      <button
        className={`${styles.toggleButton} ${props.follow ? styles.active : ''}`}
        aria-pressed={props.follow}
        title="Keep the playhead in view during playback"
        onClick={() => props.onFollow(!props.follow)}
      >
        Follow
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
        <select
          aria-label="Track height"
          value={props.trackSize}
          onChange={(event) => props.onTrackSize(event.target.value as TrackSize)}
        >
          <option value="compact">Compact</option>
          <option value="normal">Normal</option>
          <option value="large">Large</option>
        </select>
        <button className={styles.addTrackButton} onClick={props.onAddTrack}>
          <Icon name="plus" /> Audio Track
        </button>
      </div>
    </header>
  );
}
