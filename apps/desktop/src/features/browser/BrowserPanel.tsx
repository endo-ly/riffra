import { useState } from 'react';
import type { LibraryAsset, PluginEntry, RecordingAsset, Track } from '@/model/domain';
import type { InboxController } from '@/features/library/hooks/useInbox';
import type { useInstrumentLibrary } from '@/features/instruments/hooks/useInstrumentLibrary';
import { InstrumentBrowserSection } from '@/features/instruments/InstrumentBrowserSection';
import { LibrarySearchSection } from '@/features/library/LibrarySearchSection';
import { PluginBrowserSection } from '@/features/plugins/PluginBrowserSection';
import { RecordingBrowserSection } from '@/features/recording/RecordingBrowserSection';
import { Icon } from '@/shared/ui/primitives';
import styles from './BrowserPanel.module.css';
import { BrowserSection } from './BrowserSection';

export interface BrowserPanelProps {
  projectSwitching?: boolean;
  safeMode?: boolean;
  library: {
    query: string;
    setQuery: (query: string) => void;
    results: LibraryAsset[];
    searchQuery: string;
    selectedAsset: LibraryAsset | null;
    relatedAssets: LibraryAsset[];
    onSelectAsset: (asset: LibraryAsset) => void;
    onPreviewAsset: () => void;
    onUpdateAsset: (tag: string | null, note: string | null) => void;
    onImportMidi: () => void;
  };
  plugins: {
    plugins: PluginEntry[];
    visiblePlugins: PluginEntry[];
    selectedTrack: Track | null;
    onAddPlugin: (plugin: PluginEntry, target: 'instrument' | 'effect') => void;
  };
  instruments?: ReturnType<typeof useInstrumentLibrary>;
  onApplyInstrument?: (instrumentId: string) => void;
  recordings: {
    visibleRecordings: RecordingAsset[];
    count: number;
  };
  inbox: InboxController;
}

export function BrowserPanel({
  library,
  plugins,
  instruments,
  onApplyInstrument = () => undefined,
  recordings,
  inbox,
  projectSwitching = false,
  safeMode = false,
}: BrowserPanelProps) {
  const [expanded, setExpanded] = useState({ Instruments: true, Plugins: true, Recordings: true });

  const toggleSection = (section: 'Instruments' | 'Plugins' | 'Recordings') =>
    setExpanded((current) => ({ ...current, [section]: !current[section] }));

  return (
    <aside className={styles.libraryPanel} aria-label="Browser" data-library-panel>
      <div className={styles.toolbar}>
        <label className={styles.search}>
          <Icon name="search" />
          <input
            aria-label="Browser search"
            value={library.query}
            onChange={(event) => library.setQuery(event.target.value)}
            placeholder="Search"
          />
        </label>
        <button
          type="button"
          className={styles.toolButton}
          aria-label="Import MIDI"
          title="Import MIDI"
          onClick={() => void library.onImportMidi()}
        >
          <Icon name="import" />
        </button>
        <button
          type="button"
          className={styles.toolButton}
          aria-label="Find duplicates"
          title="Find duplicates"
          onClick={() => void inbox.detectDuplicates().catch(() => undefined)}
        >
          <Icon name="copy" />
        </button>
      </div>
      <div className={styles.libraryContent}>
        <LibrarySearchSection library={library} />
        {instruments && (
          <BrowserSection
            label="Instruments"
            count={instruments.items.length}
            open={expanded.Instruments}
            onToggle={() => toggleSection('Instruments')}
          >
            <InstrumentBrowserSection
              controller={instruments}
              selectedTrack={plugins.selectedTrack}
              projectSwitching={projectSwitching}
              safeMode={safeMode}
              onApply={onApplyInstrument}
            />
          </BrowserSection>
        )}
        <BrowserSection
          label="Plugins"
          count={plugins.plugins.length}
          open={expanded.Plugins}
          onToggle={() => toggleSection('Plugins')}
        >
          <PluginBrowserSection
            plugins={plugins.plugins}
            visiblePlugins={plugins.visiblePlugins}
            selectedTrack={plugins.selectedTrack}
            projectSwitching={projectSwitching}
            onAddPlugin={plugins.onAddPlugin}
          />
        </BrowserSection>
        <BrowserSection
          label="Recordings"
          count={recordings.count}
          open={expanded.Recordings}
          onToggle={() => toggleSection('Recordings')}
        >
          <RecordingBrowserSection recordings={recordings.visibleRecordings} inbox={inbox} />
        </BrowserSection>
      </div>
    </aside>
  );
}
