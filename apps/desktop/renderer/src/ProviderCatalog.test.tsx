import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import type { ProviderCatalogItem } from './model-settings-types';
import { ProviderCatalog } from './ProviderCatalog';

const configuredProvider = (index: number): ProviderCatalogItem => ({
  catalogModelCount: index,
  catalogProviderId: `configured-${index}`,
  configurationClass: 'manual_configuration',
  configuredEndpointCount: 1,
  id: `configured-${index}`,
  initials: 'C',
  localProvider: {
    created_at: '2026-09-01T00:00:00Z',
    display_name: `Configured ${index}`,
    endpoint_order: [],
    id: `prv_01ARZ3NDEKTSV4RRFFQ69G5FA${index}`,
    slug: `configured-${index}`,
    source: { catalog_provider_id: `configured-${index}`, type: 'models_dev' },
    updated_at: '2026-09-01T00:00:00Z',
  },
  name: `Configured ${index}`,
  source: 'models.dev',
});

const catalogProvider = (index: number): ProviderCatalogItem => ({
  catalogModelCount: index,
  catalogProviderId: `catalog-${index}`,
  configurationClass: 'manual_configuration',
  configuredEndpointCount: 0,
  id: `catalog-${index}`,
  initials: 'P',
  localProvider: undefined,
  name: `Provider ${index.toString().padStart(2, '0')}`,
  source: 'models.dev',
});

describe('Provider Catalog grouping', () => {
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1_200);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  test('keeps configured Providers first and collapses the directory to three responsive rows', async () => {
    const user = userEvent.setup();
    const onConfigure = vi.fn();
    const onSelect = vi.fn();
    render(
      <ProviderCatalog
        onAddCustom={vi.fn()}
        onConfigure={onConfigure}
        onSelect={onSelect}
        providers={[
          configuredProvider(1),
          configuredProvider(2),
          ...Array.from({ length: 20 }, (_, index) => catalogProvider(index + 1)),
        ]}
        selectedProviderId="configured-1"
      />,
    );

    const headings = screen.getAllByRole('heading', { level: 4 });
    expect(headings.map((heading) => heading.textContent)).toEqual([
      '已配置 Provider',
      '未配置目录',
    ]);
    // No explanation under the heading, no section label and no catalog status line.
    expect(screen.queryByText(/目录来自 models\.dev/u)).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Provider' })).toBeNull();
    expect(screen.queryByText(/models\.dev 目录/u)).toBeNull();
    expect(screen.queryByRole('button', { name: /刷新/u })).toBeNull();
    expect(screen.getByRole('button', { name: /添加提供商/u })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^Provider 14.*尚未添加$/u })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /^Provider 15.*尚未添加$/u })).toBeNull();

    await user.click(screen.getByRole('button', { name: /Configured 1.*1 个连接/u }));
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'configured-1' }));
    expect(onConfigure).not.toHaveBeenCalled();

    const configureButton = screen.getByRole('button', { name: '配置 Configured 1' });
    configureButton.focus();
    await user.keyboard('{Enter}');
    expect(onConfigure).toHaveBeenCalledWith(expect.objectContaining({ id: 'configured-1' }));

    await user.click(screen.getByRole('button', { name: /查看更多/u }));
    expect(screen.getByRole('button', { name: /^Provider 20.*尚未添加$/u })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: /收起 Provider/u }));
    expect(screen.queryByRole('button', { name: /^Provider 15.*尚未添加$/u })).toBeNull();
  });

  test('an unavailable catalog says so where its Providers would be', () => {
    render(
      <ProviderCatalog
        catalogUnavailable
        onAddCustom={vi.fn()}
        onConfigure={vi.fn()}
        onSelect={vi.fn()}
        providers={[configuredProvider(1)]}
        selectedProviderId="configured-1"
      />,
    );
    const directory = screen.getByRole('region', { name: '未配置目录' });
    expect(directory.textContent).toContain('models.dev 目录暂不可用');
    expect(screen.getByRole('region', { name: '已配置 Provider' }).textContent).not.toContain(
      '不可用',
    );
  });
});
