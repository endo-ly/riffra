// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { InstrumentPicker } from './InstrumentPicker';

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

function renderPicker() {
  return render(
    <InstrumentPicker
      instruments={instruments}
      plugins={plugins}
      onSelectInstrument={vi.fn()}
      onSelectVst3={vi.fn()}
      onClose={vi.fn()}
    />,
  );
}

describe('InstrumentPicker', () => {
  it('shows user and built-in instruments with external candidates in one picker', () => {
    renderPicker();

    expect(screen.getByText('Instruments')).toBeInTheDocument();
    expect(
      screen.getByRole('button', {
        name: 'Clean Sub Bass — A focused low-frequency bass instrument.',
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'User Piano — A user instrument.' }),
    ).toBeInTheDocument();
    expect(screen.getByText('External Instruments')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'External Keys — Example Vendor' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'External Reverb — Example Vendor' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/Sonalloy/i)).not.toBeInTheDocument();
  });

  it('routes built-in and VST3 selections to their distinct callbacks', () => {
    const onSelectInstrument = vi.fn();
    const onSelectVst3 = vi.fn();
    render(
      <InstrumentPicker
        instruments={instruments}
        plugins={plugins}
        onSelectInstrument={onSelectInstrument}
        onSelectVst3={onSelectVst3}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Clean Sub Bass — A focused low-frequency bass instrument.',
      }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'External Keys — Example Vendor' }));

    expect(onSelectInstrument).toHaveBeenCalledWith('builtin:01-clean-sub-bass');
    expect(onSelectVst3).toHaveBeenCalledWith(plugins[0]);
  });
});
