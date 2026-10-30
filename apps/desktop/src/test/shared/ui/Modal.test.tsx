// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { Modal } from '@/shared/ui/Modal';

afterEach(cleanup);

function NestedModals() {
  const [parentOpen, setParentOpen] = useState(false);
  const [childOpen, setChildOpen] = useState(false);

  return (
    <>
      <button type="button" onClick={() => setParentOpen(true)}>
        Open parent
      </button>
      {parentOpen && (
        <Modal label="Parent" onClose={() => setParentOpen(false)} className="parent">
          <button type="button" onClick={() => setChildOpen(true)}>
            Open child
          </button>
          {childOpen && (
            <Modal label="Child" onClose={() => setChildOpen(false)} className="child">
              <button autoFocus type="button" onClick={() => setChildOpen(false)}>
                Close child
              </button>
            </Modal>
          )}
        </Modal>
      )}
    </>
  );
}

describe('Modal', () => {
  it('closes only the top modal on Escape and restores focus after each close', async () => {
    const user = userEvent.setup();
    render(<NestedModals />);

    const openParent = screen.getByRole('button', { name: 'Open parent' });
    await user.click(openParent);
    const openChild = screen.getByRole('button', { name: 'Open child' });
    await user.click(openChild);
    expect(screen.getByRole('button', { name: 'Close child' })).toHaveFocus();

    fireEvent.keyDown(window, { key: 'Escape' });

    expect(screen.queryByRole('dialog', { name: 'Child' })).not.toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'Parent' })).toBeInTheDocument();
    expect(openChild).toHaveFocus();

    fireEvent.keyDown(window, { key: 'Escape' });

    expect(screen.queryByRole('dialog', { name: 'Parent' })).not.toBeInTheDocument();
    expect(openParent).toHaveFocus();
  });
});
