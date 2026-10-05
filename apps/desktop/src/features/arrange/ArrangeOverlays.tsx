import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import overlayStyles from './WorkspaceArrangeOverlay.module.css';

export interface ArrangeConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
}

interface ArrangeMarkerRenameController {
  markerRename: { markerId: string; name: string } | null;
  saveMarkerRename: () => void;
  updateMarkerRename: (name: string) => void;
  cancelMarkerRename: () => void;
}

interface ArrangeOverlaysProps {
  ruler: ArrangeMarkerRenameController;
  contextMenu: { x: number; y: number; items: ContextMenuItem[] } | null;
  onCloseContextMenu: () => void;
  confirmRequest: ArrangeConfirmRequest | null;
  onDismissConfirm: () => void;
}

export function ArrangeOverlays(props: ArrangeOverlaysProps) {
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
