import type { ReactNode } from 'react';
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
  browserOpen: boolean;
  onToggleBrowser: () => void;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  /** Placed at the end of the toolbar, after the toggles. */
  accessory?: ReactNode;
}

export function TimelineToolbar(props: TimelineToolbarProps) {
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
          <ToolbarDivider />
          <ToolbarToggle
            active={props.inspectorOpen}
            icon="inspector"
            ariaLabel="Inspector"
            title="Show or hide Inspector (Ctrl+I)"
            onClick={props.onToggleInspector}
          >
            Inspector
          </ToolbarToggle>
        </>
      }
    >
      <ToolbarToggle
        active={props.browserOpen}
        icon="folder"
        ariaLabel="Library"
        title="Show or hide Library (Ctrl+B)"
        onClick={props.onToggleBrowser}
      >
        Library
      </ToolbarToggle>
      <ToolbarDivider />
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
      <ToolbarDivider />
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
      {props.accessory}
    </Toolbar>
  );
}
