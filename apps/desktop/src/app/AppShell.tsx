import { useSession } from '@/shared/session/session-context';
import { selectActiveProjectId, selectHostReady } from '@/shared/session/selectors';
import { useEffect, useState } from 'react';
import { useAppShortcuts } from '@/app/useAppShortcuts';
import { useSessionRuntime } from '@/app/runtime/useSessionRuntime';
import { useRuntimeProjectionToast } from '@/app/runtime/useRuntimeProjectionToast';
import { useBrowserPlacement } from '@/app/workspace/useBrowserPlacement';
import { EditorWorkspace } from '@/app/workspace/EditorWorkspace';
import { PropertiesPanel } from '@/features/inspector/PropertiesPanel';
import { BrowserPanel } from '@/features/library/BrowserPanel';
import { MissingDependencies } from '@/features/project/MissingDependencies';
import { AudioSettingsDialog } from '@/features/audio/AudioSettingsDialog';
import { ToastStack } from '@/shared/ui/ToastStack';
import { BootScreen } from './layout/BootScreen';
import { GlobalControlBar } from './layout/GlobalControlBar';
import { ProjectHostSelector } from './layout/ProjectHostSelector';
import { ProjectBoundRegion } from './layout/ProjectBoundRegion';
import { ShellNotices } from './layout/ShellNotices';
import { LeftColumn } from './layout/LeftColumn';
import { CommandPalette } from './layout/CommandPalette';
import { useAppUpdater } from '@/app/runtime/useAppUpdater';
import styles from './App.module.css';
import shellStyles from './AppShell.module.css';

export function AppShell() {
  useSessionRuntime();
  useRuntimeProjectionToast();
  const [audioSettingsOpen, setAudioSettingsOpen] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);
  useAppShortcuts(setCommandOpen);
  const sessionReady = useSession((state) => Boolean(state.project.boot?.canonical.session));
  const activeProjectId = useSession(selectActiveProjectId);
  const bootstrapError = useSession((state) => state.project.bootstrapError);
  const bootstrapLoading = useSession((state) => state.project.bootstrapLoading);
  const retryBootstrap = useSession((state) => state.project.bootstrap);
  const exportProject = useSession((state) => state.project.exportPackage);
  const importProject = useSession((state) => state.project.importPackage);
  const missingCount = useSession((state) => state.missing.items.length);
  const hostReason = useSession((state) => state.host.connection.reason);
  const hostConnected = useSession(selectHostReady);
  const applyBrowserItem = useBrowserPlacement();
  const appUpdater = useAppUpdater();

  useEffect(() => {
    if (!hostConnected) {
      setAudioSettingsOpen(false);
      setCommandOpen(false);
    }
  }, [hostConnected, setAudioSettingsOpen, setCommandOpen]);

  const sessionSelector = <ProjectHostSelector />;

  if (!sessionReady)
    return (
      <BootScreen
        selector={sessionSelector}
        connected={hostConnected}
        reason={hostReason}
        error={bootstrapError}
        loading={bootstrapLoading}
        onRetry={retryBootstrap}
      />
    );

  const commandItems = [
    {
      section: 'PROJECT',
      label: 'Import Project…',
      description: 'Import a .riffra project package',
      run: () => void importProject(),
    },
    {
      section: 'PROJECT',
      label: 'Export Project…',
      description: 'Save a .riffra project package',
      run: () => void exportProject(),
    },
    {
      section: 'SETTINGS',
      label: 'Check for Updates',
      description: 'Check GitHub Releases for a newer Riffra',
      run: () => void appUpdater.checkNow(),
    },
    {
      section: 'SETTINGS',
      label: 'Audio Settings',
      description: 'Configure driver and Windows devices',
      run: () => setAudioSettingsOpen(true),
    },
  ];
  return (
    <main className={shellStyles.appShell}>
      <GlobalControlBar
        onOpenCommand={() => setCommandOpen(true)}
        onOpenAudioSettings={() => setAudioSettingsOpen(true)}
        audioSettingsOpen={audioSettingsOpen}
      />

      {audioSettingsOpen && hostConnected && (
        <AudioSettingsDialog onClose={() => setAudioSettingsOpen(false)} />
      )}

      <ShellNotices appUpdater={appUpdater} />

      {missingCount > 0 && (
        <div className={!hostConnected ? styles.hostDisconnected : undefined}>
          <MissingDependencies />
        </div>
      )}

      <LeftColumn
        browser={<BrowserPanel onApply={(placement) => void applyBrowserItem(placement)} />}
        properties={
          <ProjectBoundRegion>
            <PropertiesPanel />
          </ProjectBoundRegion>
        }
      >
        <section className={shellStyles.workspace}>
          <ProjectBoundRegion>
            <EditorWorkspace key={activeProjectId ?? 'no-project'} />
          </ProjectBoundRegion>
        </section>
      </LeftColumn>

      {commandOpen && hostConnected && (
        <CommandPalette items={commandItems} onClose={() => setCommandOpen(false)} />
      )}

      <ToastStack />
    </main>
  );
}
