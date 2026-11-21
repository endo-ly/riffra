// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { BrowserPanel } from '@/features/library/BrowserPanel';
import type {
  CreativeSession,
  InstrumentLibraryItem,
  LibraryAsset,
  PluginEntry,
  RecordingAsset,
  Track,
} from '@/model/generated';
import { createFakeNativeApi, type FakeNativeApi } from '@/test/support/fake-native-api';
import { createTestSessionStore, renderWithSession } from '@/test/support/session-provider';
import { defaultSession } from '@/native/preview/fixtures';
import type { BrowserPlacement } from '@/model/placement';
import { RIFFRA_PLUGIN_MIME } from '@/shared/browser-drag';

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
  identifier: null,
  format: 'VST3',
  role: 'effect',
  path: 'C:\\VST3\\SpaceVerb.vst3',
  bundle: true,
  modifiedAtMs: null,
  scanState: 'validated',
};

const synth: PluginEntry = { ...verb, id: 'plugin:synth', name: 'Wave Synth', role: 'instrument' };
const unclassified: PluginEntry = {
  ...verb,
  id: 'plugin:unknown',
  name: 'Unknown Plug-in',
  role: null,
};

const sourceAsset: LibraryAsset = {
  id: 'asset:source',
  name: 'Source Asset',
  kind: 'audio',
  path: null,
  tag: null,
  note: null,
  createdAtMs: null,
  updatedAtMs: null,
  stability: 'stable',
};
const targetAsset: LibraryAsset = {
  ...sourceAsset,
  id: 'asset:target',
  name: 'Target Asset',
};

const take = {
  id: 'recording:C:\\inbox\\take-a',
  path: 'C:\\inbox\\take-a.wav',
  name: 'Take A',
  state: 'completed',
  error: null,
  sampleRate: 44_100,
  samplesWritten: 44_100,
  rawAssetId: null,
  processedAssetId: null,
  midiAssetId: null,
} as RecordingAsset;

const keysTrack: Track = {
  externalAudioSourceTrackIds: [],
  panLaw: 'equalPower',
  id: 'track:keys',
  name: 'Keys',
  kind: 'instrument',
  gainDb: 0,
  pan: 0,
  muted: false,
  solo: false,
  armed: false,
  monitoring: 'off',
  midiInput: {},
  effects: [],
};

function browserSession(track: Track | null): CreativeSession {
  const session = defaultSession();
  if (track) session.arrangement.tracks = [track];
  return session;
}

async function renderBrowser({
  api = createFakeNativeApi({ instruments: [bass] }),
  selectedTrack = null,
  onApply = vi.fn(),
  plugins = [verb],
}: {
  api?: FakeNativeApi;
  selectedTrack?: Track | null;
  onApply?: (placement: BrowserPlacement) => void;
  plugins?: PluginEntry[];
} = {}) {
  const store = createTestSessionStore(api, { session: browserSession(selectedTrack) });
  store.getState().catalog.acceptPlugins(plugins);
  await store.getState().catalog.reloadInstruments();
  await store.getState().recording.reload();
  if (selectedTrack)
    store.getState().workspace.setSelection({ kind: 'track', trackId: selectedTrack.id });
  renderWithSession(<BrowserPanel onApply={onApply} />, { api, store });
  return { api, store, onApply };
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
    const { onApply } = await renderBrowser();

    // Act
    await user.click(treeItem('Bass'));
    await user.dblClick(treeItem('Clean Sub Bass'));

    // Assert
    expect(onApply).toHaveBeenCalledWith({
      kind: 'instrument',
      instrumentId: 'builtin:bass',
      name: 'Clean Sub Bass',
    });
    expect(screen.getByRole('button', { name: 'Load on new Track' })).toBeInTheDocument();
  });

  it('offers effect plug-ins for a selected Instrument Track', async () => {
    // Arrange
    const user = userEvent.setup();
    const { onApply } = await renderBrowser({ selectedTrack: keysTrack });

    // Act
    await user.click(treeItem('Effects'));
    await user.click(treeItem('Space Verb'));
    await user.click(screen.getByRole('button', { name: 'Add to Keys' }));

    // Assert
    expect(onApply).toHaveBeenCalledWith({
      kind: 'effectPlugin',
      pluginPath: 'C:\\VST3\\SpaceVerb.vst3',
      name: 'Space Verb',
    });
  });

  it('opens instrument and effect plug-ins from their rows instead of previewing them', async () => {
    // Arrange
    const user = userEvent.setup();
    const api = createFakeNativeApi({ instruments: [bass], plugins: [verb, synth] });
    const openPlugin = vi.spyOn(api, 'openPluginAudition');
    await renderBrowser({ api, plugins: [verb, synth] });
    await user.click(treeItem('Plug-ins'));

    // Act
    await user.click(treeItem('Wave Synth'));
    await user.click(screen.getByRole('button', { name: 'Open Wave Synth' }));
    await user.click(treeItem('Effects'));
    await user.click(treeItem('Space Verb'));
    await user.click(screen.getByRole('button', { name: 'Open Space Verb' }));

    // Assert
    expect(openPlugin).toHaveBeenNthCalledWith(1, synth.path);
    expect(openPlugin).toHaveBeenNthCalledWith(2, verb.path);
  });

  it('only offers Open for a validated plug-in with a classified role', async () => {
    // Arrange
    const user = userEvent.setup();
    await renderBrowser({ plugins: [unclassified] });
    await user.click(treeItem('Unclassified Plug-ins'));

    // Act
    await user.click(treeItem('Unknown Plug-in'));

    // Assert
    expect(screen.queryByRole('button', { name: 'Open Unknown Plug-in' })).not.toBeInTheDocument();
  });

  it('previews and edits the asset from the invoked row', async () => {
    // Arrange
    const user = userEvent.setup();
    const api = createFakeNativeApi({
      instruments: [bass],
      responses: {
        searchLibrary: [sourceAsset, targetAsset],
        updateLibraryAsset: (id: string, tag: string | null) => ({ ...targetAsset, id, tag }),
      },
    });
    const previewAsset = vi.spyOn(api, 'previewAsset');
    const updateLibraryAsset = vi.spyOn(api, 'updateLibraryAsset');
    await renderBrowser({ api });
    await user.type(screen.getByRole('textbox', { name: 'Browser search' }), 'Asset');
    await screen.findByRole('treeitem', { name: /^Target Asset/ });
    await user.click(treeItem('Source Asset'));

    // Act
    const targetRow = treeItem('Target Asset');
    await user.hover(targetRow);
    await user.click(screen.getByRole('button', { name: 'Preview Target Asset' }));
    fireEvent.contextMenu(treeItem('Target Asset'));
    await user.click(screen.getByRole('menuitem', { name: 'Preview' }));
    fireEvent.contextMenu(treeItem('Target Asset'));
    await user.click(screen.getByRole('menuitem', { name: 'Edit tag…' }));
    await user.type(screen.getByRole('textbox', { name: 'Asset tag' }), 'Kit{Enter}');

    // Assert
    expect(previewAsset).toHaveBeenNthCalledWith(1, 'asset:target', {});
    expect(previewAsset).toHaveBeenNthCalledWith(2, 'asset:target', {});
    expect(updateLibraryAsset).toHaveBeenCalledWith('asset:target', 'Kit', null);
  });

  it('drags a plug-in so it can be dropped on a Track', async () => {
    // Arrange
    const user = userEvent.setup();
    await renderBrowser();
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
    const api = createFakeNativeApi({ instruments: [bass], recordings: [take] });
    const deleteRecording = vi.spyOn(api, 'deleteRecording');
    await renderBrowser({ api });
    await user.click(treeItem('Recordings'));

    // Act
    fireEvent.contextMenu(treeItem('Take A'));
    await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    expect(deleteRecording).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Delete' }));

    // Assert
    expect(deleteRecording).toHaveBeenCalledWith(take.id);
  });

  it('renames a take in place from its context menu', async () => {
    // Arrange
    const user = userEvent.setup();
    const api = createFakeNativeApi({
      instruments: [bass],
      recordings: [take],
      responses: { renameRecording: () => take.id },
    });
    const renameRecording = vi.spyOn(api, 'renameRecording');
    await renderBrowser({ api });
    await user.click(treeItem('Recordings'));
    await user.click(treeItem('Take A'));

    // Act
    await user.pointer({ keys: '[MouseRight]', target: treeItem('Take A') });
    await user.click(screen.getByRole('menuitem', { name: 'Rename…' }));
    const name = screen.getByRole('textbox', { name: 'Rename Take A' });
    await user.clear(name);
    await user.type(name, 'Verse take{Enter}');

    // Assert
    expect(renameRecording).toHaveBeenCalledWith(take.id, 'Verse take');
  });

  it('edits the tags of the selected instrument in place', async () => {
    // Arrange
    const user = userEvent.setup();
    const api = createFakeNativeApi({ instruments: [bass] });
    const setInstrumentUserTags = vi.spyOn(api, 'setInstrumentUserTags');
    await renderBrowser({ api });
    await user.click(treeItem('Bass'));
    await user.click(treeItem('Clean Sub Bass'));

    // Act
    await user.pointer({ keys: '[MouseRight]', target: treeItem('Clean Sub Bass') });
    await user.click(screen.getByRole('menuitem', { name: 'Edit tags…' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Tags for Clean Sub Bass' }),
      'Dark, Wide{Enter}',
    );

    // Assert
    expect(setInstrumentUserTags).toHaveBeenCalledWith(bass.id, ['Dark', 'Wide']);
  });
});
