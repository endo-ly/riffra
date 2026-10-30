import { TestSessionProvider } from '@/test/support/session-provider';
import { createFakeNativeApi } from '@/test/support/fake-native-api';
// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PluginPicker } from '@/features/devices/PluginPicker';

afterEach(cleanup);

const instruments = [
  {
    id: 'builtin:01-clean-sub-bass',
    name: 'Clean Sub Bass',
    description: 'A focused low-frequency bass instrument.',
    origin: 'builtIn' as const,
  },
  {
    id: 'user:01a0b3d5-b173-7ac1-a7e4-686e99599f94',
    name: 'User Piano',
    description: 'A user instrument.',
    origin: 'user' as const,
  },
];

const plugins = [
  {
    id: 'plugin:keys',
    name: 'External Keys',
    vendor: 'Example Vendor',
    version: null,
    format: 'VST3' as const,
    role: 'instrument' as const,
    path: 'C:\\Plugins\\ExternalKeys.vst3',
    bundle: true,
    modifiedAtMs: null,
    scanState: 'validated' as const,
  },
  {
    id: 'plugin:reverb',
    name: 'External Reverb',
    vendor: 'Example Vendor',
    version: null,
    format: 'VST3' as const,
    role: 'effect' as const,
    path: 'C:\\Plugins\\ExternalReverb.vst3',
    bundle: true,
    modifiedAtMs: null,
    scanState: 'validated' as const,
  },
];

describe('PluginPicker', () => {
  it('routes library and external instrument selections to their distinct callbacks', () => {
    const onSelectInstrument = vi.fn();
    const onSelectPlugin = vi.fn();
    render(
      <TestSessionProvider api={createFakeNativeApi()}>
        <PluginPicker
          title="Choose Instrument"
          role="instrument"
          plugins={plugins}
          instruments={instruments}
          onSelectInstrument={onSelectInstrument}
          onSelectPlugin={onSelectPlugin}
          onClose={vi.fn()}
        />
      </TestSessionProvider>,
    );

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Clean Sub Bass — A focused low-frequency bass instrument.',
      }),
    );
    expect(onSelectInstrument).toHaveBeenCalledWith('builtin:01-clean-sub-bass');

    fireEvent.click(screen.getByRole('button', { name: 'User Piano — A user instrument.' }));
    expect(onSelectInstrument).toHaveBeenCalledWith(instruments[1].id);

    fireEvent.click(screen.getByRole('button', { name: 'External Keys — Example Vendor' }));
    expect(onSelectPlugin).toHaveBeenCalledWith(plugins[0]);
    expect(screen.queryByRole('button', { name: 'External Reverb — Example Vendor' })).toBeNull();
  });

  it('filters plug-ins by name or vendor and omits instrument sections for effect selection', () => {
    const onSelectPlugin = vi.fn();
    render(
      <TestSessionProvider api={createFakeNativeApi()}>
        <PluginPicker
          title="Add Effect"
          role="effect"
          plugins={plugins}
          onSelectPlugin={onSelectPlugin}
          onClose={vi.fn()}
        />
      </TestSessionProvider>,
    );

    expect(screen.queryByText('Instruments')).not.toBeInTheDocument();
    expect(screen.queryByText('External Instruments')).not.toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText('Search plugins...'), {
      target: { value: 'example vendor' },
    });
    const reverb = screen.getByRole('button', { name: 'External Reverb — Example Vendor' });
    fireEvent.click(reverb);

    expect(onSelectPlugin).toHaveBeenCalledWith(plugins[1]);
    expect(screen.queryByRole('button', { name: 'External Keys — Example Vendor' })).toBeNull();
  });
});
