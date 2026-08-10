// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { InspectorPanel } from '@/components/layout/InspectorPanel';
import { defaultSession, type BootstrapState } from '@/lib/domain';
import { FakeNativeApi, fakeAudioStatus } from '@/native/native-api-fake';

afterEach(cleanup);

function renderPanel(selection: Parameters<typeof InspectorPanel>[0]['arrangeSelection']) {
  const session = defaultSession();
  session.workspace = 'arrange';
  session.arrangement.tracks.push({
    id: 'track:audio',
    name: 'Audio',
    kind: 'audio',
    gainDb: 0,
    pan: 0,
    muted: false,
    solo: false,
    armed: false,
    monitoring: 'off',
    midiInput: {},
    rack: { devices: [], macros: [] },
  });
  const api = new FakeNativeApi({ bootstrapState: { session } });
  const boot: BootstrapState = {
    session,
    pluginCatalog: [],
    runtimeStarted: true,
    runtimeStartupFinished: true,
    recoveredFromGeneration: false,
    safeMode: false,
    nativeAvailable: true,
    recoveryCandidates: [],
    dataRoot: 'C:\\Riffra',
    vst3Root: 'C:\\VST3',
  };
  render(
    <InspectorPanel
      audio={fakeAudioStatus()}
      boot={boot}
      focusMode={false}
      setFocusMode={() => undefined}
      session={session}
      setSession={() => undefined}
      arrangeSelection={selection}
      setArrangeSelection={() => undefined}
      missingDependencies={[]}
      plugins={[]}
      onDisableMissingPlugin={async () => undefined}
      onReplaceMissingPlugin={async () => undefined}
      onRescanMissingPlugins={async () => undefined}
      api={api}
    />,
  );
}

describe('InspectorPanel', () => {
  it('shows the selected Track title without a close button', () => {
    renderPanel({ kind: 'track', trackId: 'track:audio' });

    expect(screen.getAllByText('TRACK')[0]).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Close Inspector/ })).not.toBeInTheDocument();
  });

  it('shows the empty Arrange title without a clip selection', () => {
    renderPanel({ kind: 'none' });

    expect(screen.getByText('INSPECTOR')).toBeInTheDocument();
  });
});
