import { SNAP_GRID_OPTIONS, snapGridLabel, type SnapGrid } from '@/model/clip-grid';
import type { TimelineTool } from '@/features/timeline/model/timeline-layout';
import {
  Toolbar,
  ToolbarDivider,
  ToolbarSegmented,
  ToolbarSelect,
  ToolbarStepper,
  ToolbarToggle,
} from '@/shared/ui/Toolbar';

const ZOOM_STEP = 1.25;

interface TimelineToolbarProps {
  tool: TimelineTool;
  snap: SnapGrid;
  zoom: number;
  rulerMode: 'bars' | 'time';
  onTool: (tool: TimelineTool) => void;
  onSnap: (snap: SnapGrid) => void;
  onZoom: (zoom: number) => void;
  onRulerMode: (mode: 'bars' | 'time') => void;
  automationAvailable: boolean;
  automationOpen: boolean;
  onToggleAutomation: () => void;
  playSurfaceOpen: boolean;
  onTogglePlaySurface: () => void;
  mixerOpen: boolean;
  onToggleMixer: () => void;
  computerKeyboardOn: boolean;
  /** What the computer keyboard plays while it is on: an octave such as `C4`, or `Pads`. */
  computerKeyboardRange: string;
  /** Why live input cannot reach the focused Track, or `null` when it can. */
  computerKeyboardBlockedReason: string | null;
  onToggleComputerKeyboard: () => void;
}

export function TimelineToolbar(props: TimelineToolbarProps) {
  // One source of truth, so the toggle state and its label can never disagree.
  const computerKeyboardOn =
    props.computerKeyboardOn && props.computerKeyboardBlockedReason === null;

  return (
    <Toolbar
      label="Arrange toolbar"
      trailing={
        <>
          <ToolbarSegmented
            label="Ruler display"
            value={props.rulerMode}
            onChange={props.onRulerMode}
            options={[
              { value: 'bars', label: 'Bars' },
              { value: 'time', label: 'Time' },
            ]}
          />
          <ToolbarStepper
            ariaLabel="Timeline zoom"
            valueText={`${Math.round(props.zoom * 100)}%`}
            onStep={(direction) =>
              props.onZoom(props.zoom * (direction > 0 ? ZOOM_STEP : 1 / ZOOM_STEP))
            }
          />
        </>
      }
    >
      <ToolbarSegmented
        label="Arrange tool"
        value={props.tool}
        onChange={props.onTool}
        options={[
          { value: 'select', label: 'Select', icon: 'pointer' },
          { value: 'split', label: 'Split', icon: 'scissors' },
        ]}
      />
      <ToolbarDivider />
      <ToolbarSelect
        label="Snap"
        value={props.snap}
        onChange={props.onSnap}
        options={SNAP_GRID_OPTIONS.map((value) => ({ value, label: snapGridLabel(value) }))}
      />
      <ToolbarToggle
        active={props.automationOpen}
        icon="curve"
        ariaLabel="Automation"
        disabled={!props.automationAvailable}
        title={
          props.automationAvailable
            ? 'Show or hide Automation for the selected Track'
            : 'Select a Track to edit Automation'
        }
        onClick={props.onToggleAutomation}
      >
        Automation
      </ToolbarToggle>
      <ToolbarToggle
        active={props.mixerOpen}
        icon="mixer"
        ariaLabel="Mixer"
        title="Show or hide the Mixer"
        onClick={props.onToggleMixer}
      >
        Mixer
      </ToolbarToggle>
      <ToolbarToggle
        active={props.playSurfaceOpen}
        icon="keys"
        ariaLabel="Play Surface"
        title="Show or hide the on-screen keyboard and drum pads"
        onClick={props.onTogglePlaySurface}
      >
        Play Surface
      </ToolbarToggle>
      <ToolbarToggle
        active={computerKeyboardOn}
        icon="computerKeyboard"
        ariaLabel="Computer keyboard input"
        title={
          props.computerKeyboardBlockedReason === null
            ? 'Play the focused Instrument Track from the computer keyboard (Ctrl+Shift+K)'
            : `${props.computerKeyboardBlockedReason} Computer keyboard input stays off.`
        }
        onClick={props.onToggleComputerKeyboard}
      >
        {computerKeyboardOn ? props.computerKeyboardRange : undefined}
      </ToolbarToggle>
    </Toolbar>
  );
}
