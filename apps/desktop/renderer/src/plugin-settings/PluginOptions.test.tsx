import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { PluginView } from '@aura/contracts/generated';
import { PluginOptions } from './PluginOptions';

afterEach(cleanup);

// A secret setting saved with nothing typed asks for it instead of doing nothing.

test('saving an empty secret setting asks for it and sends nothing', () => {
  const onSetSecret = vi.fn();
  const plugin = {
    options: [
      {
        option: { key: 'secret', title: 'App Secret', kind: 'secret', required: true },
        set: false,
      },
    ],
  } as unknown as PluginView;
  render(
    <PluginOptions
      plugin={plugin}
      busy={false}
      onSaveText={vi.fn()}
      onSetSecret={onSetSecret}
      onClearSecret={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: '保存 App Secret' }));
  expect(screen.getByRole('alert').textContent).toBe('请先填写 App Secret。');
  expect(onSetSecret).not.toHaveBeenCalled();
});
