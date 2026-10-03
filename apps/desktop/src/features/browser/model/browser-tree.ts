import type {
  AssetId,
  InstrumentCollection,
  InstrumentLibraryItem,
  LibraryAsset,
  PluginEntry,
  RecordingAsset,
} from '@/model/domain';
import type { BrowserPlacement } from '@/features/arrange/model/browser-placement';
import { matchesInstrumentQuery } from '@/features/instruments/model/instrument-library';

export type BrowserItem =
  | { kind: 'instrument'; key: string; instrument: InstrumentLibraryItem }
  | { kind: 'plugin'; key: string; plugin: PluginEntry }
  | { kind: 'recording'; key: string; recording: RecordingAsset }
  | { kind: 'asset'; key: string; asset: LibraryAsset };

export interface BrowserFolder {
  kind: 'folder';
  key: string;
  label: string;
  children: BrowserNode[];
  itemCount: number;
  /** Set on a user collection, which the user can rename or delete. */
  collectionId?: number;
}

export type BrowserNode = BrowserFolder | BrowserItem;

interface BrowserSources {
  instruments: InstrumentLibraryItem[];
  /** The Host's filing categories, in display order. */
  categories: string[];
  collections: InstrumentCollection[];
  plugins: PluginEntry[];
  recordings: RecordingAsset[];
}

interface BrowserSearchResult {
  item: BrowserItem;
  location: string;
}

const instrumentItem = (instrument: InstrumentLibraryItem): BrowserItem => ({
  kind: 'instrument',
  key: `instrument:${instrument.id}`,
  instrument,
});
const pluginItem = (plugin: PluginEntry): BrowserItem => ({
  kind: 'plugin',
  key: `plugin:${plugin.id}`,
  plugin,
});
const recordingItem = (recording: RecordingAsset): BrowserItem => ({
  kind: 'recording',
  key: `recording:${recording.id}`,
  recording,
});
const assetItem = (asset: LibraryAsset): BrowserItem => ({
  kind: 'asset',
  key: `asset:${asset.id}`,
  asset,
});

function folder(
  key: string,
  label: string,
  children: BrowserNode[],
  collectionId?: number,
): BrowserFolder {
  const itemCount = children.reduce(
    (count, child) => count + (child.kind === 'folder' ? child.itemCount : 1),
    0,
  );
  return { kind: 'folder', key, label, children, itemCount, collectionId };
}

const byName = (left: { name: string }, right: { name: string }) =>
  left.name.localeCompare(right.name);

/**
 * Builds the Browser tree. Categories, collections, and favorites are folders,
 * so one Instrument can appear in several places; plug-ins sit with the
 * instruments or effects they provide. Empty folders are omitted, except the
 * user's own collections.
 */
export function buildBrowserTree(sources: BrowserSources): BrowserFolder[] {
  const categoryFolders = sources.categories.map((category) =>
    folder(
      `instruments/${category}`,
      category,
      sources.instruments
        .filter((instrument) => instrument.category === category)
        .map(instrumentItem),
    ),
  );
  const plugins = [...sources.plugins].sort(byName);
  const pluginsWithRole = (role: PluginEntry['role']) =>
    plugins.filter((plugin) => plugin.role === role).map(pluginItem);

  return [
    folder(
      'favorites',
      'Favorites',
      sources.instruments.filter((instrument) => instrument.favorite).map(instrumentItem),
    ),
    folder(
      'collections',
      'Collections',
      sources.collections.map((collection) =>
        folder(
          `collections/${collection.id}`,
          collection.name,
          sources.instruments
            .filter((instrument) => instrument.collectionIds.includes(collection.id))
            .map(instrumentItem),
          collection.id,
        ),
      ),
    ),
    folder('instruments', 'Instruments', [
      ...categoryFolders,
      folder('instruments/plug-ins', 'Plug-ins', pluginsWithRole('instrument')),
    ]),
    folder('effects', 'Effects', pluginsWithRole('effect')),
    folder('unclassified', 'Unclassified Plug-ins', pluginsWithRole(null)),
    folder('recordings', 'Recordings', sources.recordings.map(recordingItem)),
  ]
    .map(withoutEmptyFolders)
    .filter((node): node is BrowserFolder => node !== null);
}

function withoutEmptyFolders(node: BrowserFolder): BrowserFolder | null {
  const keepEmpty = node.collectionId !== undefined;
  const children = node.children
    .map((child) => (child.kind === 'folder' ? withoutEmptyFolders(child) : child))
    .filter((child): child is BrowserNode => child !== null);
  if (!keepEmpty && children.length === 0) return null;
  return folder(node.key, node.label, children, node.collectionId);
}

/**
 * Lists every item matching the query, ignoring folders. `assets` are the
 * Library's own search results for the same query; those that belong to a
 * listed Recording are left out so a take appears once.
 */
export function searchBrowserItems(
  sources: BrowserSources & { assets: LibraryAsset[] },
  query: string,
): BrowserSearchResult[] {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return [];
  const matches = (...values: (string | null)[]) =>
    values.some((value) => value?.toLocaleLowerCase().includes(normalized));
  const recordingAssetIds = new Set<string>(
    sources.recordings.flatMap((recording) =>
      [recording.rawAssetId, recording.processedAssetId, recording.midiAssetId].filter(
        (id): id is AssetId => id !== null,
      ),
    ),
  );
  return [
    ...sources.instruments
      .filter((instrument) => matchesInstrumentQuery(instrument, normalized, sources.collections))
      .map((instrument) => ({
        item: instrumentItem(instrument),
        location: `Instruments › ${instrument.category}`,
      })),
    ...[...sources.plugins]
      .sort(byName)
      .filter((plugin) => matches(plugin.name, plugin.vendor))
      .map((plugin) => ({ item: pluginItem(plugin), location: pluginLocation(plugin) })),
    ...sources.recordings
      .filter((recording) => matches(recording.name))
      .map((recording) => ({ item: recordingItem(recording), location: 'Recordings' })),
    ...sources.assets
      .filter((asset) => !recordingAssetIds.has(asset.id))
      .map((asset) => ({ item: assetItem(asset), location: 'Assets' })),
  ];
}

function pluginLocation(plugin: PluginEntry): string {
  if (plugin.role === 'instrument') return 'Instruments › Plug-ins';
  if (plugin.role === 'effect') return 'Effects';
  return 'Unclassified Plug-ins';
}

export function browserItemName(item: BrowserItem): string {
  switch (item.kind) {
    case 'instrument':
      return item.instrument.name;
    case 'plugin':
      return item.plugin.name;
    case 'recording':
      return item.recording.name;
    case 'asset':
      return item.asset.name;
  }
}

/** One short line that tells similar items apart. */
export function browserItemDetail(item: BrowserItem): string {
  switch (item.kind) {
    case 'instrument':
      return item.instrument.tags.slice(0, 3).join(', ');
    case 'plugin':
      if (item.plugin.scanState !== 'validated') return `Plug-in ${item.plugin.scanState}`;
      return item.plugin.vendor ?? 'VST3';
    case 'recording':
      return recordingDetail(item.recording);
    case 'asset':
      return item.asset.kind === 'midi' ? 'MIDI' : item.asset.kind === 'audio' ? 'Audio' : '';
  }
}

function recordingDetail(recording: RecordingAsset): string {
  if (recording.error) return 'Unreadable';
  if (recording.state !== 'completed') return capitalize(recording.state);
  if (!recording.sampleRate || recording.samplesWritten === 0) return 'Empty';
  return formatDuration(recording.samplesWritten / recording.sampleRate);
}

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds - minutes * 60).toFixed(1).padStart(4, '0')}`;
}

function capitalize(value: string): string {
  return value.charAt(0).toLocaleUpperCase() + value.slice(1);
}

/** How the item enters the Arrangement, or null when it cannot be placed. */
export function browserItemPlacement(item: BrowserItem): BrowserPlacement | null {
  switch (item.kind) {
    case 'instrument':
      return { kind: 'instrument', instrumentId: item.instrument.id, name: item.instrument.name };
    case 'plugin':
      if (item.plugin.scanState !== 'validated' || item.plugin.role === null) return null;
      return {
        kind: item.plugin.role === 'instrument' ? 'instrumentPlugin' : 'effectPlugin',
        pluginPath: item.plugin.path,
        name: item.plugin.name,
      };
    case 'recording': {
      const assetId = item.recording.processedAssetId ?? item.recording.rawAssetId;
      if (!assetId || item.recording.error) return null;
      return { kind: 'audioAsset', assetId, name: item.recording.name };
    }
    case 'asset':
      if (item.asset.kind !== 'audio' && item.asset.kind !== 'midi') return null;
      return {
        kind: item.asset.kind === 'audio' ? 'audioAsset' : 'midiAsset',
        assetId: item.asset.id as AssetId,
        name: item.asset.name,
      };
  }
}
