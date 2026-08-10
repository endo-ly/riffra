// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { LibraryPanel } from '@/components';
import type { InboxController } from '@/hooks/useInbox';
import type { LibraryAsset, PluginEntry, RecordingAsset, Track } from '@/lib/domain';

// This suite verifies LibraryPanel's callback wiring only. useInbox/FakeNativeApi
// behavior is covered separately in src/test/hooks/useInbox.test.tsx.

const recordingA: RecordingAsset = {
  id: 'recording:C:\\inbox\\take-a',
  name: 'Take A',
  path: 'C:\\inbox\\take-a',
  state: 'completed',
  error: null,
  startedAt: null,
  updatedAt: null,
  rawFile: 'raw.wav',
  processedFile: 'processed.wav',
  rawPath: 'C:\\inbox\\take-a\\raw.wav',
  processedPath: 'C:\\inbox\\take-a\\processed.wav',
  rawAssetId: null,
  processedAssetId: null,
  midiAssetId: null,
  capture: null,
  midiFile: null,
  sampleRate: 44_100,
  samplesWritten: 44_100,
  droppedBlocks: 0,
  missingSamples: 0,
  dropoutStartSample: null,
  dropoutEndSample: null,
  rawAttemptedSamples: 44_100,
  processedAttemptedSamples: 44_100,
  rawDroppedBlocks: 0,
  processedDroppedBlocks: 0,
  rawMissingSamples: 0,
  processedMissingSamples: 0,
  rawDropoutStartSample: null,
  rawDropoutEndSample: null,
  processedDropoutStartSample: null,
  processedDropoutEndSample: null,
  recoveryStatus: 'clean',
};

const recordingB: RecordingAsset = {
  ...recordingA,
  id: 'recording:C:\\inbox\\take-b',
  name: 'Take B',
  path: 'C:\\inbox\\take-b',
  rawPath: 'C:\\inbox\\take-b\\raw.wav',
  processedPath: 'C:\\inbox\\take-b\\processed.wav',
};

function makeInbox(): InboxController {
  return {
    selectedId: recordingA.id,
    setSelectedId: vi.fn(),
    selected: recordingA,
    duplicateGroups: [],
    duplicateIds: new Set([recordingA.id, recordingB.id]),
    message: '1 duplicate group found (2 recordings).',
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

const libraryStub = {
  section: 'Recordings',
  setSection: vi.fn(),
  query: '',
  setQuery: vi.fn(),
  results: [] as LibraryAsset[],
  searchQuery: '',
  selectedAsset: null,
  relatedAssets: [] as LibraryAsset[],
  onSelectAsset: vi.fn(),
  onPreviewAsset: vi.fn(),
  onEditAsset: vi.fn(),
  onOpenInDesign: vi.fn(),
  onImportMidi: vi.fn(),
};

const rackStub = {
  plugins: [] as PluginEntry[],
  visiblePlugins: [] as PluginEntry[],
  selectedTrack: null,
  onAddPlugin: vi.fn(),
};

const recordingsStub = {
  visibleRecordings: [recordingA, recordingB],
  count: 2,
  onOpenRecording: vi.fn(),
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('Inbox preservation zone (LIB-003)', () => {
  it('adds a plugin to the selected instrument track from its add button', async () => {
    const plugin: PluginEntry = {
      id: 'plug:example',
      name: 'Example Synth',
      vendor: 'Acme',
      version: null,
      format: 'VST3',
      path: 'C:\\VST3\\example.vst3',
      bundle: false,
      modifiedAtMs: null,
      scanState: 'validated',
    };
    const user = userEvent.setup();
    const onAddPlugin = vi.fn();
    render(
      <LibraryPanel
        library={{ ...libraryStub, section: 'Plugins' }}
        rack={{
          ...rackStub,
          plugins: [plugin],
          visiblePlugins: [plugin],
          onAddPlugin,
          selectedTrack: {
            id: 'track:instrument',
            name: 'Instrumental',
            kind: 'instrument',
            gainDb: 0,
            pan: 0,
            muted: false,
            solo: false,
            armed: false,
            monitoring: 'off',
            midiInput: {},
            rack: { devices: [], macros: [] },
          } satisfies Track,
        }}
        recordings={recordingsStub}
        inbox={makeInbox()}
      />,
    );

    fireEvent.click(screen.getByText('Example Synth'));
    expect(onAddPlugin).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: /Example Synth/ }));
    expect(onAddPlugin).toHaveBeenCalledWith(plugin, 'instrument');
  });

  it('adds an effect to the selected Audio Track from its add button', async () => {
    // Arrange
    const plugin: PluginEntry = {
      id: 'plug:effect',
      name: 'Example Effect',
      vendor: 'Acme',
      version: null,
      format: 'VST3',
      path: 'C:\\VST3\\effect.vst3',
      bundle: false,
      modifiedAtMs: null,
      scanState: 'validated',
    };
    const user = userEvent.setup();
    const onAddPlugin = vi.fn();
    render(
      <LibraryPanel
        library={{ ...libraryStub, section: 'Plugins' }}
        rack={{
          ...rackStub,
          plugins: [plugin],
          visiblePlugins: [plugin],
          onAddPlugin,
          selectedTrack: {
            id: 'track:audio',
            name: 'Audio Track',
            kind: 'audio',
            gainDb: 0,
            pan: 0,
            muted: false,
            solo: false,
            armed: false,
            monitoring: 'off',
            midiInput: {},
            rack: { devices: [], macros: [] },
          } satisfies Track,
        }}
        recordings={recordingsStub}
        inbox={makeInbox()}
      />,
    );

    // Act
    await user.click(screen.getByRole('button', { name: /Example Effect/ }));

    // Assert
    expect(onAddPlugin).toHaveBeenCalledWith(plugin, 'effect');
  });

  it('exposes every Inbox operation for the selected take', async () => {
    const inbox = makeInbox();
    const user = userEvent.setup();
    render(
      <LibraryPanel
        library={libraryStub}
        rack={rackStub}
        recordings={recordingsStub}
        inbox={inbox}
      />,
    );

    expect(screen.getByLabelText('Find duplicates')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('1 duplicate group found');
    const selectA = screen.getByLabelText(`Select ${recordingA.name}`);
    expect(selectA).toBeInTheDocument();
    expect(selectA.closest('.recording-row')).not.toHaveClass('plugin-row');
    // Duplicate takes are flagged for the user.
    expect(selectA.closest('.recording-row')).toHaveClass('duplicate');

    await user.click(screen.getByLabelText('Find duplicates'));
    expect(inbox.detectDuplicates).toHaveBeenCalledTimes(1);

    await user.click(screen.getByLabelText('Preview'));
    expect(inbox.preview).toHaveBeenCalledWith(recordingA);

    await user.click(screen.getByLabelText('Promote'));
    expect(inbox.promote).toHaveBeenCalledWith(recordingA.id);

    await user.click(screen.getByLabelText('Archive'));
    expect(inbox.archive).toHaveBeenCalledWith(recordingA.id);

    vi.spyOn(window, 'confirm').mockReturnValue(true);
    await user.click(screen.getByLabelText('Delete'));
    expect(inbox.remove).toHaveBeenCalledWith(recordingA.id);

    await user.click(screen.getByLabelText('Analyze'));
    expect(recordingsStub.onOpenRecording).toHaveBeenCalledWith(recordingA);
  });

  it('renames and tags the selected take through prompts', async () => {
    const inbox = makeInbox();
    const prompt = vi.fn().mockReturnValueOnce('Renamed Take').mockReturnValueOnce('mytag');
    vi.stubGlobal('prompt', prompt);
    const user = userEvent.setup();
    render(
      <LibraryPanel
        library={libraryStub}
        rack={rackStub}
        recordings={recordingsStub}
        inbox={inbox}
      />,
    );

    await user.click(screen.getByLabelText('Rename'));
    expect(inbox.rename).toHaveBeenCalledWith(recordingA.id, 'Renamed Take');

    await user.click(screen.getByLabelText('Tag'));
    expect(inbox.tag).toHaveBeenCalledWith(recordingA.id, 'mytag', null);
  });

  it('does not select a take that failed to index', () => {
    const broken = { ...recordingA, error: 'missing audio' };
    const inbox = makeInbox();
    inbox.selected = broken;
    inbox.selectedId = broken.id;
    render(
      <LibraryPanel
        library={libraryStub}
        rack={rackStub}
        recordings={{ ...recordingsStub, visibleRecordings: [broken] }}
        inbox={inbox}
      />,
    );
    expect(screen.getByLabelText(`Select ${broken.name}`)).toHaveAttribute('aria-disabled', 'true');
  });

  it('shows an Inbox operation error instead of a success message', () => {
    const inbox = makeInbox();
    inbox.message = null;
    inbox.error = 'The audio engine is offline.';
    render(
      <LibraryPanel
        library={libraryStub}
        rack={rackStub}
        recordings={recordingsStub}
        inbox={inbox}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('The audio engine is offline.');
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('exposes completed Audio as a draggable Arrange Asset', () => {
    const recording = {
      ...recordingA,
      processedAssetId:
        'asset:018f85b9-5fe1-7ef2-91d8-e6b4e665d41a' as RecordingAsset['processedAssetId'],
    };
    const setData = vi.fn();
    render(
      <LibraryPanel
        library={libraryStub}
        rack={rackStub}
        recordings={{ ...recordingsStub, visibleRecordings: [recording] }}
        inbox={makeInbox()}
      />,
    );

    const row = screen.getByLabelText(`Select ${recording.name}`);
    expect(row).toHaveAttribute('draggable', 'true');
    fireEvent.dragStart(row, { dataTransfer: { effectAllowed: '', setData } });

    expect(setData).toHaveBeenCalledWith(
      'application/x-riffra-asset',
      expect.stringContaining(`"assetId":"${recording.processedAssetId}"`),
    );
  });
});
