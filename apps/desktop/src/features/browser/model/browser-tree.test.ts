import { describe, expect, it } from 'vitest';
import type {
  InstrumentLibraryItem,
  LibraryAsset,
  PluginEntry,
  RecordingAsset,
} from '@/model/domain';
import { toAssetId } from '@/native/contracts';
import {
  buildBrowserTree,
  searchBrowserItems,
  type BrowserFolder,
  type BrowserNode,
} from './browser-tree';

function instrument(
  id: string,
  name: string,
  category: string,
  extra: Partial<InstrumentLibraryItem> = {},
): InstrumentLibraryItem {
  return {
    id,
    presetId: null,
    origin: 'builtIn',
    name,
    author: null,
    description: null,
    defaultCategory: category,
    category,
    defaultTags: [],
    userTags: [],
    tags: [],
    favorite: false,
    collectionIds: [],
    recommendedRange: null,
    preview: null,
    ...extra,
  };
}

function plugin(name: string, role: PluginEntry['role']): PluginEntry {
  return {
    id: `plugin:${name}`,
    name,
    vendor: 'Vendor',
    version: '1.0',
    format: 'VST3',
    role,
    path: `C:\\VST3\\${name}.vst3`,
    bundle: true,
    modifiedAtMs: null,
    scanState: 'validated',
  };
}

const take = {
  id: 'recording:take',
  name: 'Bass take',
  rawAssetId: toAssetId('asset:raw'),
  processedAssetId: null,
  midiAssetId: null,
} as RecordingAsset;

function labels(nodes: BrowserNode[]): string[] {
  return nodes.map((node) => (node.kind === 'folder' ? node.label : node.key));
}

function child(folder: BrowserFolder | undefined, label: string): BrowserFolder | undefined {
  return folder?.children.find(
    (node): node is BrowserFolder => node.kind === 'folder' && node.label === label,
  );
}

describe('buildBrowserTree', () => {
  it("files instruments in the Host's category order and plug-ins by role, leaving out empty folders", () => {
    // Arrange
    const sources = {
      instruments: [
        instrument('pad', 'Glass Pad', 'Pad', { favorite: true }),
        instrument('odd', 'Odd Noise', 'Other'),
        instrument('bass', 'Sub Bass', 'Bass'),
      ],
      categories: ['Pad', 'Bass', 'Keys', 'Other'],
      collections: [{ id: 1, name: 'Sketches' }],
      plugins: [plugin('Verb', 'effect'), plugin('Synth', 'instrument')],
      recordings: [],
    };

    // Act
    const tree = buildBrowserTree(sources);
    const instruments = tree.find((folder) => folder.key === 'instruments');

    // Assert
    expect(labels(tree)).toEqual(['Favorites', 'Collections', 'Instruments', 'Effects']);
    expect(labels(instruments!.children)).toEqual(['Pad', 'Bass', 'Other', 'Plug-ins']);
    expect(instruments!.itemCount).toBe(4);
    expect(labels(child(instruments, 'Plug-ins')!.children)).toEqual(['plugin:plugin:Synth']);
    expect(child(tree[1], 'Sketches')?.itemCount).toBe(0);
  });
});

describe('searchBrowserItems', () => {
  it('lists matches from every source once, with where they live', () => {
    // Arrange
    const sources = {
      instruments: [instrument('bass', 'Sub Bass', 'Bass'), instrument('pad', 'Glass Pad', 'Pad')],
      categories: ['Bass', 'Pad', 'Other'],
      collections: [],
      plugins: [plugin('Bass Amp', 'effect')],
      recordings: [take],
      assets: [
        { id: 'asset:raw', name: 'Bass take', kind: 'audio' },
        { id: 'asset:loop', name: 'Bass loop', kind: 'audio' },
      ] as LibraryAsset[],
    };

    // Act
    const results = searchBrowserItems(sources, 'bass');

    // Assert
    expect(results.map((result) => [result.item.key, result.location])).toEqual([
      ['instrument:bass', 'Instruments › Bass'],
      ['plugin:plugin:Bass Amp', 'Effects'],
      ['recording:recording:take', 'Recordings'],
      ['asset:asset:loop', 'Assets'],
    ]);
  });
});
