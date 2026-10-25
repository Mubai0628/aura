import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';

import { PreviewPicture } from './PicturePreview';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test('zoom holds the point under the pointer, limits scale, pans and resets to fit', async () => {
  const user = userEvent.setup();
  render(<PreviewPicture name="view.png" src="data:image/png;base64,AA==" />);
  await user.click(screen.getByRole('button', { name: '预览 view.png' }));
  const preview = screen.getByRole('dialog', { name: '图片预览' });
  const image = within(preview).getByRole('img');
  vi.spyOn(preview, 'getBoundingClientRect').mockReturnValue({
    left: 0,
    top: 0,
    width: 800,
    height: 600,
  } as DOMRect);
  const capture = vi.fn();
  image.setPointerCapture = capture;
  vi.stubGlobal(
    'PointerEvent',
    class extends MouseEvent {
      readonly pointerId = 1;
    },
  );
  fireEvent.wheel(preview, { deltaY: -Math.log(2) / 0.002, clientX: 500, clientY: 350 });
  expect(image.style.transform).toBe('translate(-100px, -50px) scale(2)');
  // The image point 100 px right, 50 px below centre is still at the pointer after zoom.
  fireEvent.pointerDown(image, { button: 0, clientX: 500, clientY: 350 });
  fireEvent.pointerMove(image, { clientX: 530, clientY: 370 });
  fireEvent.pointerUp(image);
  expect(capture).toHaveBeenCalledWith(1);
  expect(image.style.transform).toBe('translate(-70px, -30px) scale(2)');
  fireEvent.wheel(preview, { deltaY: -10000 });
  expect(image.style.transform).toContain('scale(8)');
  fireEvent.wheel(preview, { deltaY: 10000 });
  expect(image.style.transform).toContain('scale(0.1)');
  fireEvent.doubleClick(image);
  expect(image.style.transform).toBe('translate(0px, 0px) scale(1)');
});

test('Tab stays in the preview, image clicks keep it open and clicking the shade closes it', async () => {
  const user = userEvent.setup();
  render(<PreviewPicture name="view.png" src="data:image/png;base64,AA==" />);
  const trigger = screen.getByRole('button', { name: '预览 view.png' });
  await user.click(trigger);
  const preview = screen.getByRole('dialog', { name: '图片预览' });
  await user.keyboard('{Tab}{Shift>}{Tab}{/Shift}');
  expect(document.activeElement).toBe(screen.getByRole('button', { name: '关闭图片预览' }));
  await user.click(within(preview).getByRole('img'));
  expect(screen.getByRole('dialog')).toBe(preview);
  await user.click(preview);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(document.activeElement).toBe(trigger);
});
