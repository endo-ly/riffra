// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { LibraryPanel } from '@/components';
import type { InboxController } from '@/hooks/useInbox';
import type { LibraryAsset, PluginEntry, RecordingAsset } from '@/lib/domain';

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
  midiFile: null,
  midiPath: null,
  sampleRate: 44_100,
  samplesWritten: 44_100,
  droppedBlocks: 0,
  missingSamples: 0,
  dropoutStartSample: null,
  dropoutEndSample: null,
  recoveryStatus: 'clean',
  provenance: null,
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
};

const rackStub = {
  plugins: [] as PluginEntry[],
  visiblePlugins: [] as PluginEntry[],
  onLoadPlugin: vi.fn(),
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
    expect(screen.getByLabelText(`Select ${broken.name}`)).toBeDisabled();
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
});
