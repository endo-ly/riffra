// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ArrangeToolbar } from './ArrangeToolbar';

afterEach(cleanup);

describe('ArrangeToolbar', () => {
  it('keeps global and track controls out of the timeline toolbar', () => {
    render(
      <ArrangeToolbar
        tool="select"
        snap="bar"
        zoom={1}
        rulerMode="bars"
        onTool={vi.fn()}
        onSnap={vi.fn()}
        onZoom={vi.fn()}
        onRulerMode={vi.fn()}
        automationAvailable
        automationOpen={false}
        onToggleAutomation={vi.fn()}
      />,
    );

    expect(screen.queryByLabelText('Project BPM')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Time signature')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Track height')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Add track')).not.toBeInTheDocument();
  });
});
