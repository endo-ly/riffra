import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import type { WorkspaceConfirmRequest } from './hooks/useTimelineTrackOperations';
import overlayStyles from './TimelineOverlays.module.css';

interface WorkspaceMarkerRenameController {
  markerRename: { markerId: string; name: string } | null;
  saveMarkerRename: () => void;
  updateMarkerRename: (name: string) => void;
  cancelMarkerRename: () => void;
}

interface TimelineOverlaysProps {
  ruler: WorkspaceMarkerRenameController;
  contextMenu: { x: number; y: number; items: ContextMenuItem[] } | null;
  onCloseContextMenu: () => void;
  confirmRequest: WorkspaceConfirmRequest | null;
  onDismissConfirm: () => void;
}

export function TimelineOverlays(props: TimelineOverlaysProps) {
  return (
    <>
      {props.ruler.markerRename && (
        <form
          className={overlayStyles.markerDialog}
          aria-label="Rename marker"
          onSubmit={(event) => {
            event.preventDefault();
            props.ruler.saveMarkerRename();
          }}
        >
          <strong>Rename Marker</strong>
          <label>
            <span>Name</span>
            <input
              autoFocus
              value={props.ruler.markerRename.name}
              onChange={(event) => props.ruler.updateMarkerRename(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Escape') props.ruler.cancelMarkerRename();
              }}
            />
          </label>
          <div>
            <button type="button" onClick={props.ruler.cancelMarkerRename}>
              Cancel
            </button>
            <button type="submit">Save</button>
          </div>
        </form>
      )}

      {props.confirmRequest && (
        <ConfirmDialog
          title={props.confirmRequest.title}
          message={props.confirmRequest.message}
          confirmLabel={props.confirmRequest.confirmLabel}
          danger={props.confirmRequest.danger}
          onConfirm={props.confirmRequest.onConfirm}
          onCancel={props.onDismissConfirm}
        />
      )}

      {props.contextMenu && (
        <ContextMenu
          x={props.contextMenu.x}
          y={props.contextMenu.y}
          items={props.contextMenu.items}
          onClose={props.onCloseContextMenu}
        />
      )}
    </>
  );
}
