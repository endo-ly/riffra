import { useSession, useSessionContext } from '@/session/session-context';
import { selectHostReady } from '@/session/selectors';
import { useEffect, useState } from 'react';
import { useAppShortcuts } from '@/app/hooks/useAppShortcuts';
import { useSessionRuntime } from '@/session/hooks/useSessionRuntime';
import { usePlaceBrowserItem } from '@/features/timeline/hooks/usePlaceBrowserItem';
import { EditorWorkspace } from '@/app/workspace/EditorWorkspace';
import { PropertiesPanel } from '@/features/inspector/PropertiesPanel';
import { BrowserPanel } from '@/features/library/BrowserPanel';
import { AudioSettingsDialog } from '@/features/audio/AudioSettingsDialog';
import { ToastStack } from '@/shared/ui/message/ToastStack';
import { BootScreen } from '@/app/layout/BootScreen';
import { GlobalControlBar } from '@/app/layout/GlobalControlBar';
import { ProjectHostSelector } from '@/features/project/ProjectHostSelector';
import { ProjectBoundRegion } from '@/app/layout/ProjectBoundRegion';
import { NoticeBar } from '@/app/status/NoticeBar';
import { WorkspaceWait } from '@/app/status/WorkspaceWait';
import { AppBodyLayout } from '@/app/layout/AppBodyLayout';
import { CommandPalette } from '@/app/layout/CommandPalette';
import { ShowcaseWorkspace } from '@/features/showcase/ShowcaseWorkspace';
import shellStyles from './AppShell.module.css';

export function AppShell() {
  useSessionRuntime();
  const { commit } = useSessionContext();
  const [audioSettingsOpen, setAudioSettingsOpen] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);
  useAppShortcuts(setCommandOpen);
  const sessionReady = useSession((state) => Boolean(state.project.boot?.canonical.session));
  const bootstrapError = useSession((state) => state.project.bootstrapError);
  const bootstrapLoading = useSession((state) => state.project.bootstrapLoading);
  const retryBootstrap = useSession((state) => state.project.bootstrap);
  const exportProject = useSession((state) => state.project.exportPackage);
  const importProject = useSession((state) => state.project.importPackage);
  const hostReason = useSession((state) => state.host.connection.reason);
  const hostConnected = useSession(selectHostReady);
  const viewMode = useSession((state) => state.workspace.viewMode);
  const toggleViewMode = useSession((state) => state.workspace.toggleViewMode);
  const { placeAtPlayhead } = usePlaceBrowserItem(commit);
  const checkForUpdate = useSession((state) => state.appUpdate.checkNow);
  const checkForUpdateOnStartup = useSession((state) => state.appUpdate.checkOnStartup);

  useEffect(() => {
    void checkForUpdateOnStartup();
  }, [checkForUpdateOnStartup]);

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
      section: 'VIEW',
      label: viewMode === 'showcase' ? 'Switch to Edit' : 'Switch to Showcase',
      description:
        viewMode === 'showcase'
          ? 'Return to the arrangement and its editors'
          : 'Watch the arrangement perform full screen',
      run: toggleViewMode,
    },
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
      run: () => void checkForUpdate(),
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

      <NoticeBar />

      {viewMode === 'showcase' ? (
        <div className={shellStyles.showcaseBody}>
          <ProjectBoundRegion>
            <ShowcaseWorkspace />
          </ProjectBoundRegion>
        </div>
      ) : (
        <AppBodyLayout
          browser={<BrowserPanel onApply={(placement) => void placeAtPlayhead(placement)} />}
          properties={
            <ProjectBoundRegion>
              <PropertiesPanel />
            </ProjectBoundRegion>
          }
        >
          <section className={shellStyles.workspace}>
            <ProjectBoundRegion>
              <EditorWorkspace />
            </ProjectBoundRegion>
          </section>
        </AppBodyLayout>
      )}

      <WorkspaceWait />

      {commandOpen && hostConnected && (
        <CommandPalette items={commandItems} onClose={() => setCommandOpen(false)} />
      )}

      <ToastStack />
    </main>
  );
}
