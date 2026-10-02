// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { BrowserPanel, type BrowserPanelProps } from '@/features/browser/BrowserPanel';
import type { InboxController } from '@/features/library/hooks/useInbox';
import type {
  InstrumentLibraryItem,
  LibraryAsset,
  PluginEntry,
  RecordingAsset,
  Track,
} from '@/model/domain';
import { RIFFRA_PLUGIN_MIME } from '@/shared/plugin-drag';

const bass: InstrumentLibraryItem = {
  id: 'builtin:bass',
  presetId: 'bass',
  origin: 'builtIn',
  name: 'Clean Sub Bass',
  author: null,
  description: null,
  defaultCategory: 'Bass',
  category: 'Bass',
  defaultTags: ['Sub'],
  userTags: [],
  tags: ['Sub'],
  favorite: false,
  collectionIds: [],
  recommendedRange: null,
  preview: null,
};

const verb: PluginEntry = {
  id: 'plugin:verb',
  name: 'Space Verb',
  vendor: 'Echo Labs',
  version: '1.0',
  format: 'VST3',
  role: 'effect',
  path: 'C:\\VST3\\SpaceVerb.vst3',
  bundle: true,
  modifiedAtMs: null,
  scanState: 'validated',
};

const take = {
  id: 'recording:C:\\inbox\\take-a',
  name: 'Take A',
  state: 'completed',
  error: null,
  sampleRate: 44_100,
  samplesWritten: 44_100,
  rawAssetId: null,
  processedAssetId: null,
  midiAssetId: null,
} as RecordingAsset;

const keysTrack = { id: 'track:keys', name: 'Keys', kind: 'instrument' } as Track;

function inbox(): InboxController {
  return {
    selectedId: null,
    setSelectedId: vi.fn(),
    selected: null,
    duplicateGroups: [],
    duplicateIds: new Set(),
    message: null,
    error: null,
    rename: vi.fn().mockResolvedValue(undefined),
    remove: vi.fn().mockResolvedValue(undefined),
    archive: vi.fn().mockResolvedValue(undefined),
    promote: vi.fn().mockResolvedValue(undefined),
    tag: vi.fn().mockResolvedValue(null),
    preview: vi.fn().mockResolvedValue(undefined),
    detectDuplicates: vi.fn().mockResolvedValue(undefined),
  };
}

function renderBrowser(overrides: Partial<BrowserPanelProps> = {}) {
  const props: BrowserPanelProps = {
    query: '',
    onQueryChange: vi.fn(),
    library: {
      results: [] as LibraryAsset[],
      selectedAsset: null,
      relatedAssets: [],
      onSelectAsset: vi.fn(),
      onPreviewAsset: vi.fn(),
      onUpdateAsset: vi.fn(),
      onImportMidi: vi.fn(),
    },
    instruments: {
      items: [bass],
      collections: [],
      previewingId: null,
      previewPendingId: null,
      loading: false,
      error: null,
      toggleFavorite: vi.fn(),
      setCategory: vi.fn(),
      setTags: vi.fn(),
      createCollection: vi.fn(),
      renameCollection: vi.fn(),
      deleteCollection: vi.fn(),
      setCollectionMembership: vi.fn(),
      preview: vi.fn(),
      reload: vi.fn(),
    },
    plugins: [verb],
    recordings: [take],
    inbox: inbox(),
    selectedTrack: null,
    onApply: vi.fn(),
    ...overrides,
  };
  render(<BrowserPanel {...props} />);
  return props;
}

const treeItem = (name: string) => screen.getByRole('treeitem', { name: new RegExp(`^${name}`) });

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('BrowserPanel', () => {
  it('opens a category folder and loads an instrument on a new Track by double-click', async () => {
    // Arrange
    const user = userEvent.setup();
    const props = renderBrowser();

    // Act
    await user.click(treeItem('Bass'));
    await user.dblClick(treeItem('Clean Sub Bass'));

    // Assert
    expect(props.onApply).toHaveBeenCalledWith({
      kind: 'instrument',
      instrumentId: 'builtin:bass',
      name: 'Clean Sub Bass',
    });
    expect(screen.getByRole('button', { name: 'Load on new Track' })).toBeInTheDocument();
  });

  it('offers effect plug-ins for a selected Instrument Track', async () => {
    // Arrange
    const user = userEvent.setup();
    const props = renderBrowser({ selectedTrack: keysTrack });

    // Act
    await user.click(treeItem('Effects'));
    await user.click(treeItem('Space Verb'));
    await user.click(screen.getByRole('button', { name: 'Add to Keys' }));

    // Assert
    expect(props.onApply).toHaveBeenCalledWith({
      kind: 'effectPlugin',
      pluginPath: 'C:\\VST3\\SpaceVerb.vst3',
      name: 'Space Verb',
    });
  });

  it('lists search matches from every source with where they live', () => {
    // Arrange / Act
    renderBrowser({ query: 'a' });

    // Assert
    const results = within(screen.getByRole('tree', { name: 'Search results' }));
    expect(results.getByRole('treeitem', { name: /Clean Sub Bass/ })).toHaveTextContent(
      'Instruments › Bass',
    );
    expect(results.getByRole('treeitem', { name: /Space Verb/ })).toHaveTextContent('Effects');
    expect(results.getByRole('treeitem', { name: /Take A/ })).toHaveTextContent('Recordings');
  });

  it('drags a plug-in so it can be dropped on a Track', async () => {
    // Arrange
    const user = userEvent.setup();
    renderBrowser();
    await user.click(treeItem('Effects'));
    const setData = vi.fn();

    // Act
    fireEvent.dragStart(treeItem('Space Verb'), { dataTransfer: { setData } });

    // Assert
    expect(setData).toHaveBeenCalledWith(
      RIFFRA_PLUGIN_MIME,
      JSON.stringify({
        version: 1,
        pluginPath: 'C:\\VST3\\SpaceVerb.vst3',
        name: 'Space Verb',
        role: 'effect',
      }),
    );
  });

  it('deletes a recording from its context menu only after confirmation', async () => {
    // Arrange
    const user = userEvent.setup();
    const props = renderBrowser();
    await user.click(treeItem('Recordings'));

    // Act
    fireEvent.contextMenu(treeItem('Take A'));
    await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    expect(props.inbox.remove).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Delete' }));

    // Assert
    expect(props.inbox.remove).toHaveBeenCalledWith(take.id);
  });

  it('renames the selected take from its details', async () => {
    // Arrange
    const user = userEvent.setup();
    const props = renderBrowser();
    await user.click(treeItem('Recordings'));
    await user.click(treeItem('Take A'));
    await user.click(screen.getByRole('button', { name: /Take A/, expanded: false }));

    // Act
    const name = screen.getByRole('textbox', { name: 'Rename Take A' });
    await user.clear(name);
    await user.type(name, 'Verse take{Enter}');

    // Assert
    expect(props.inbox.rename).toHaveBeenCalledWith(take.id, 'Verse take');
  });
});
