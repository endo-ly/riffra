import type { AssetId, DesktopViewState, DesignTool, Workspace } from '@/lib/domain';
import { invokeOrFallback } from '../invoke';

export async function openAssetInDesign(
  assetId: AssetId,
  tool: DesignTool,
): Promise<DesktopViewState | null> {
  return invokeOrFallback<DesktopViewState | null>('open_asset_in_design', { assetId, tool }, null);
}

export async function switchWorkspace(workspace: Workspace): Promise<DesktopViewState | null> {
  return invokeOrFallback<DesktopViewState | null>('switch_workspace', { workspace }, null);
}
