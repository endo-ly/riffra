import { SNAP_GRID_OPTIONS, snapGridLabel, type SnapGrid } from '@/model/clip-grid';
import {
  Toolbar,
  ToolbarButton,
  ToolbarDivider,
  ToolbarSegmented,
  ToolbarSelect,
  ToolbarSlider,
  ToolbarToggle,
} from '@/shared/ui/Toolbar';
import styles from './MidiEditorPanel.module.css';

interface MidiEditorToolbarProps {
  clipName: string;
  tool: 'pointer' | 'draw';
  onToolChange: (tool: 'pointer' | 'draw') => void;
  snap: SnapGrid;
  onSnapChange: (snap: SnapGrid) => void;
  snapTicks: number;
  hasQuantizeTargets: boolean;
  hasSelectedNotes: boolean;
  hasSelectedIds: boolean;
  onQuantize: () => void;
  velocityDraft: number;
  onVelocityChange: (value: number) => void;
  onVelocityCommit: (value: number) => void;
  previewEnabled: boolean;
  previewAvailable: boolean;
  onTogglePreview: () => void;
  ghostsVisible: boolean;
  onToggleGhosts: () => void;
}

export function MidiEditorToolbar({
  clipName,
  tool,
  onToolChange,
  snap,
  onSnapChange,
  snapTicks,
  hasQuantizeTargets,
  hasSelectedNotes,
  hasSelectedIds,
  onQuantize,
  velocityDraft,
  onVelocityChange,
  onVelocityCommit,
  previewEnabled,
  previewAvailable,
  onTogglePreview,
  ghostsVisible,
  onToggleGhosts,
}: MidiEditorToolbarProps) {
  return (
    <Toolbar label="MIDI Editor toolbar">
      <span className={styles.clipIdentity} title={clipName}>
        {clipName}
      </span>
      <ToolbarDivider />
      <ToolbarSegmented
        label="MIDI Editor tool"
        value={tool}
        onChange={onToolChange}
        options={[
          { value: 'pointer', label: 'Pointer', icon: 'pointer' },
          { value: 'draw', label: 'Draw', icon: 'pencil' },
        ]}
      />
      <ToolbarDivider />
      <ToolbarSelect
        label="Snap"
        value={snap}
        onChange={onSnapChange}
        options={SNAP_GRID_OPTIONS.map((value) => ({ value, label: snapGridLabel(value) }))}
      />
      <ToolbarButton
        icon="quantize"
        ariaLabel="Quantize"
        disabled={!hasQuantizeTargets || snapTicks === 0}
        title={
          snapTicks === 0
            ? 'Select a snap grid before quantizing'
            : hasSelectedNotes
              ? 'Quantize the selected notes to the snap grid'
              : 'Quantize all notes to the snap grid'
        }
        onClick={onQuantize}
      >
        Quantize
      </ToolbarButton>
      <ToolbarDivider />
      <ToolbarSlider
        label="Vel"
        ariaLabel={hasSelectedIds ? 'Selected MIDI note velocity' : 'New MIDI note velocity'}
        value={velocityDraft}
        min={1}
        max={127}
        onChange={onVelocityChange}
        onCommit={onVelocityCommit}
      />
      <ToolbarDivider />
      <ToolbarToggle
        icon="speaker"
        ariaLabel="Preview"
        active={previewEnabled}
        disabled={!previewAvailable}
        title={
          previewAvailable ? 'Preview notes on the active instrument' : 'Audio runtime unavailable'
        }
        onClick={onTogglePreview}
      />
      <ToolbarToggle
        icon="ghostNotes"
        ariaLabel="Reference notes"
        active={ghostsVisible}
        title="Show notes from other tracks behind this clip"
        onClick={onToggleGhosts}
      />
    </Toolbar>
  );
}
