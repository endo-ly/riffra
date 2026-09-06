// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { InstrumentPicker } from './InstrumentPicker';

afterEach(cleanup);

const builtInInstruments = [
  {
    id: '01-clean-sub-bass',
    name: 'Clean Sub Bass',
    description: 'A focused low-frequency bass instrument.',
  },
];

const plugins = [
  {
    id: 'plugin:keys',
    name: 'External Keys',
    vendor: 'Example Vendor',
    version: null,
    format: 'VST3' as const,
    path: 'C:\\Plugins\\ExternalKeys.vst3',
    bundle: true,
    modifiedAtMs: null,
    scanState: 'validated' as const,
  },
];

function renderPicker() {
  return render(
    <InstrumentPicker
      builtInInstruments={builtInInstruments}
      plugins={plugins}
      onSelectBuiltIn={vi.fn()}
      onSelectVst3={vi.fn()}
      onClose={vi.fn()}
    />,
  );
}

describe('InstrumentPicker', () => {
  it('shows built-in and external candidates in one flat picker', () => {
    renderPicker();

    expect(screen.getByText('Built-in Instruments')).toBeInTheDocument();
    expect(
      screen.getByRole('button', {
        name: 'Clean Sub Bass — A focused low-frequency bass instrument.',
      }),
    ).toBeInTheDocument();
    expect(screen.getByText('External Plugins')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'External Keys — Example Vendor' }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Sonalloy/i)).not.toBeInTheDocument();
  });

  it('routes built-in and VST3 selections to their distinct callbacks', () => {
    const onSelectBuiltIn = vi.fn();
    const onSelectVst3 = vi.fn();
    render(
      <InstrumentPicker
        builtInInstruments={builtInInstruments}
        plugins={plugins}
        onSelectBuiltIn={onSelectBuiltIn}
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

    expect(onSelectBuiltIn).toHaveBeenCalledWith('01-clean-sub-bass');
    expect(onSelectVst3).toHaveBeenCalledWith(plugins[0]);
  });
});
