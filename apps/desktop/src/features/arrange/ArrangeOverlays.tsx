import type {
  ArrangementMutationResult,
  BuiltInInstrumentSummary,
  CreativeSession,
  PluginEntry,
} from '@/model/domain';
import type { ArrangeApi, JobApi } from '@/native/native-api';
import { InstrumentPicker } from './inspector/InstrumentPicker';
import { PluginPicker } from './inspector/PluginPicker';
import { ContextMenu, type ContextMenuItem } from '@/shared/ui/ContextMenu';
import { ConfirmDialog } from '@/shared/ui/ConfirmDialog';
import type { ArrangePluginPickerRequest } from './hooks/useArrangeContextMenus';
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
  api: Pick<ArrangeApi, 'setTrackBuiltInInstrument' | 'setTrackVst3Instrument' | 'addTrackEffect'> &
    Pick<JobApi, 'scanVst3Folder'>;
  plugins?: PluginEntry[];
  builtInInstruments: BuiltInInstrumentSummary[];
  commit: (operation: Promise<ArrangementMutationResult | null>) => Promise<CreativeSession | null>;
  ruler: ArrangeMarkerRenameController;
  contextMenu: { x: number; y: number; items: ContextMenuItem[] } | null;
  onCloseContextMenu: () => void;
  confirmRequest: ArrangeConfirmRequest | null;
  onDismissConfirm: () => void;
  pluginPicker: ArrangePluginPickerRequest | null;
  setPluginPicker: (picker: ArrangePluginPickerRequest | null) => void;
}

export function ArrangeOverlays(props: ArrangeOverlaysProps) {
  const { pluginPicker } = props;
  return (
    <>
      {pluginPicker &&
        (pluginPicker.kind === 'instrument' ? (
          <InstrumentPicker
            api={props.api}
            builtInInstruments={props.builtInInstruments}
            plugins={props.plugins}
            onSelectBuiltIn={(presetId) => {
              const { trackId } = pluginPicker;
              props.setPluginPicker(null);
              void props.commit(props.api.setTrackBuiltInInstrument(trackId, presetId));
            }}
            onSelectVst3={(plugin) => {
              const { trackId } = pluginPicker;
              props.setPluginPicker(null);
              void props.commit(props.api.setTrackVst3Instrument(trackId, plugin.path));
            }}
            onClose={() => props.setPluginPicker(null)}
          />
        ) : (
          <PluginPicker
            api={props.api}
            plugins={props.plugins}
            title="Add Effect"
            onSelect={(plugin) => {
              const { trackId } = pluginPicker;
              props.setPluginPicker(null);
              void props.commit(props.api.addTrackEffect(trackId, plugin.path));
            }}
            onClose={() => props.setPluginPicker(null)}
          />
        ))}

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
