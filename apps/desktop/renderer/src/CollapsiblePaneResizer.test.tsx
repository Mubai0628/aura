import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import { CollapsiblePaneResizer } from './CollapsiblePaneResizer';
import { onPaneDragEnd, paneDragging } from './pane-drag';

describe('CollapsiblePaneResizer', () => {
  const releasePointerCapture = vi.fn();
  const setPointerCapture = vi.fn();

  afterEach(cleanup);

  beforeEach(() => {
    releasePointerCapture.mockReset();
    setPointerCapture.mockReset();
    Object.defineProperties(HTMLElement.prototype, {
      hasPointerCapture: { configurable: true, value: () => true },
      releasePointerCapture: { configurable: true, value: releasePointerCapture },
      setPointerCapture: { configurable: true, value: setPointerCapture },
    });
  });

  test('keeps capture while temporarily collapsed and reopens at the minimum width', () => {
    const onCollapse = vi.fn();
    const onResize = vi.fn();
    const onResizeCommit = vi.fn();
    const onTemporaryCollapseChange = vi.fn();
    render(
      <CollapsiblePaneResizer
        className="left-panel-resizer"
        controls="left"
        direction="left"
        label="调整左侧工作区宽度"
        maximum={400}
        minimum={200}
        onCollapse={onCollapse}
        onResize={onResize}
        onResizeCommit={onResizeCommit}
        onTemporaryCollapseChange={onTemporaryCollapseChange}
        width={250}
      />,
    );
    const separator = screen.getByRole('separator');
    fireEvent.pointerDown(separator, { clientX: 300, pointerId: 1 });
    expect(setPointerCapture).toHaveBeenCalledWith(1);
    fireEvent.pointerMove(separator, { clientX: 202, pointerId: 1 });
    expect(separator.getAttribute('data-collapse-armed')).toBe('true');
    expect(onTemporaryCollapseChange).toHaveBeenLastCalledWith(true);
    expect(onCollapse).not.toHaveBeenCalled();
    expect(releasePointerCapture).not.toHaveBeenCalled();

    fireEvent.pointerMove(separator, { clientX: 250, pointerId: 1 });
    expect(onTemporaryCollapseChange).toHaveBeenLastCalledWith(false);
    expect(onResize).toHaveBeenLastCalledWith(200);
    fireEvent.pointerUp(separator, { pointerId: 1 });
    expect(onResizeCommit).toHaveBeenLastCalledWith(200);
    expect(onCollapse).not.toHaveBeenCalled();
  });

  test('commits collapse only after pointer release', () => {
    const onCollapse = vi.fn();
    const onResizeCommit = vi.fn();
    render(
      <CollapsiblePaneResizer
        className="right-panel-resizer"
        controls="right"
        direction="right"
        label="调整右侧工作区宽度"
        maximum={420}
        minimum={264}
        onCollapse={onCollapse}
        onResize={vi.fn()}
        onResizeCommit={onResizeCommit}
        width={324}
      />,
    );
    const separator = screen.getByRole('separator');
    fireEvent.pointerDown(separator, { clientX: 600, pointerId: 2 });
    fireEvent.pointerMove(separator, { clientX: 710, pointerId: 2 });
    expect(onCollapse).not.toHaveBeenCalled();
    fireEvent.pointerUp(separator, { pointerId: 2 });
    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(onResizeCommit).not.toHaveBeenCalled();
  });

  test('tells terminals a drag is in progress until the pointer is released', () => {
    const ended = vi.fn();
    const stop = onPaneDragEnd(ended);
    const view = render(
      <CollapsiblePaneResizer
        className="right-panel-resizer"
        controls="right"
        direction="right"
        label="调整右侧工作区宽度"
        maximum={420}
        minimum={264}
        onCollapse={vi.fn()}
        onResize={vi.fn()}
        onResizeCommit={vi.fn()}
        width={324}
      />,
    );
    const separator = screen.getByRole('separator');
    expect(paneDragging()).toBe(false);
    fireEvent.pointerDown(separator, { clientX: 600, pointerId: 3 });
    expect(paneDragging()).toBe(true);
    // Past the snap point and back, still pressed.
    fireEvent.pointerMove(separator, { clientX: 710, pointerId: 3 });
    fireEvent.pointerMove(separator, { clientX: 560, pointerId: 3 });
    expect(paneDragging()).toBe(true);
    expect(ended).not.toHaveBeenCalled();
    fireEvent.pointerUp(separator, { pointerId: 3 });
    expect(paneDragging()).toBe(false);
    expect(ended).toHaveBeenCalledTimes(1);
    // A divider removed in the middle of a drag does not leave terminals waiting.
    fireEvent.pointerDown(separator, { clientX: 600, pointerId: 4 });
    view.unmount();
    expect(paneDragging()).toBe(false);
    expect(ended).toHaveBeenCalledTimes(2);
    stop();
  });

  test('supports directional arrows, bounds, and Enter collapse', () => {
    const onCollapse = vi.fn();
    const onResizeCommit = vi.fn();
    render(
      <CollapsiblePaneResizer
        className="right-panel-resizer"
        controls="right"
        direction="right"
        label="调整右侧工作区宽度"
        maximum={420}
        minimum={264}
        onCollapse={onCollapse}
        onResize={vi.fn()}
        onResizeCommit={onResizeCommit}
        width={324}
      />,
    );
    const separator = screen.getByRole('separator');
    fireEvent.keyDown(separator, { key: 'ArrowLeft' });
    expect(onResizeCommit).toHaveBeenLastCalledWith(340);
    fireEvent.keyDown(separator, { key: 'Home' });
    expect(onResizeCommit).toHaveBeenLastCalledWith(264);
    fireEvent.keyDown(separator, { key: 'End' });
    expect(onResizeCommit).toHaveBeenLastCalledWith(420);
    fireEvent.keyDown(separator, { key: 'Enter' });
    expect(onCollapse).toHaveBeenCalledTimes(1);
  });
});
