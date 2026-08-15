import {
  SNAP_GRID_OPTIONS,
  snapGridLabel,
  type ArrangeTool,
  type SnapGrid,
} from '@/features/arrange/model/arrange-timeline';
import {
  Toolbar,
  ToolbarDivider,
  ToolbarSegmented,
  ToolbarSelect,
  ToolbarStepper,
  ToolbarToggle,
} from '@/shared/ui/Toolbar';

const ZOOM_STEP = 1.25;

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
        active={props.follow}
        icon="follow"
        ariaLabel="Follow"
        title="Keep the playhead in view during playback"
        onClick={() => props.onFollow(!props.follow)}
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
      />
    </Toolbar>
  );
}
