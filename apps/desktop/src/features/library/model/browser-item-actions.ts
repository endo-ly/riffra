import type {
  InstrumentLibraryItem,
  LibraryAsset,
  PluginEntry,
  RecordingAsset,
  Track,
} from '@/model/generated';
import {
  describePlacementAction,
  resolvePlacementTarget,
  type BrowserPlacement,
} from '@/shared/browser-placement';
import { browserItemPlacement, type BrowserItem } from './browser-tree';

export interface BrowserItemAction {
  label: string;
  icon: 'play' | 'stop' | 'maximize';
  active: boolean;
  pending: boolean;
  run: () => void;
}
export interface BrowserItemPlacementAction {
  placement: BrowserPlacement;
  label: string;
  available: boolean;
}
interface BrowserItemActionOptions {
  safeMode?: boolean;
  previewingId: string | null;
  previewPendingId: string | null;
  previewInstrument(item: InstrumentLibraryItem): void;
  previewRecording(item: RecordingAsset): void;
  previewAsset(item: LibraryAsset): void;
  openPlugin(item: PluginEntry): void;
}

export function placementFor(
  item: BrowserItem,
  selectedTrack: Track | null,
  projectSwitching: boolean,
): BrowserItemPlacementAction | null {
  const placement = browserItemPlacement(item);
  if (!placement) return null;
  const target = resolvePlacementTarget(placement, selectedTrack, false);
  return {
    placement,
    label: describePlacementAction(placement, target, selectedTrack?.name ?? ''),
    available: target.kind !== 'invalid' && !projectSwitching,
  };
}

export function actionFor(
  item: BrowserItem,
  options: BrowserItemActionOptions,
): BrowserItemAction | null {
  switch (item.kind) {
    case 'instrument':
      if (item.instrument.preview === null || options.safeMode) return null;
      return {
        label: options.previewingId === item.instrument.id ? 'Stop preview' : 'Preview',
        icon: options.previewingId === item.instrument.id ? 'stop' : 'play',
        active: options.previewingId === item.instrument.id,
        pending: options.previewPendingId !== null,
        run: () => void options.previewInstrument(item.instrument),
      };
    case 'recording':
      if (!browserItemPlacement(item)) return null;
      return {
        label: 'Preview',
        icon: 'play',
        active: false,
        pending: false,
        run: () => void options.previewRecording(item.recording),
      };
    case 'asset':
      if (item.asset.kind !== 'audio') return null;
      return {
        label: 'Preview',
        icon: 'play',
        active: false,
        pending: false,
        run: () => options.previewAsset(item.asset),
      };
    case 'plugin':
      // A VST3 opens like its standalone application instead of previewing.
      if (item.plugin.scanState !== 'validated' || item.plugin.role === null || options.safeMode)
        return null;
      return {
        label: 'Open',
        icon: 'maximize',
        active: false,
        pending: false,
        run: () => options.openPlugin(item.plugin),
      };
  }
}
