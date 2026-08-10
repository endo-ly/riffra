// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ArrangeToolbar } from '@/components/arrange/ArrangeToolbar';

afterEach(cleanup);

describe('ArrangeToolbar', () => {
  it('keeps side-panel controls and timebase controls out of the toolbar', () => {
    render(
      <ArrangeToolbar
        tool="select"
        snap="bar"
        zoom={1}
        trackSize="normal"
        rulerMode="bars"
        follow={false}
        onTool={vi.fn()}
        onSnap={vi.fn()}
        onZoom={vi.fn()}
        onTrackSize={vi.fn()}
        onRulerMode={vi.fn()}
        onFollow={vi.fn()}
        onAddTrack={vi.fn()}
        automationAvailable
        automationOpen={false}
        onToggleAutomation={vi.fn()}
      />,
    );

    expect(screen.queryByRole('button', { name: /Library|Inspector/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Project BPM')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Time signature')).not.toBeInTheDocument();
  });
});
