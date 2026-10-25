import { useState } from 'react';
import type {
  PackageImportDiagnosticV1,
  PackageSummaryV1,
  PackageVersionViewV1,
} from '@aura/contracts/generated';
import { usePackages } from './usePackages';
import { PackageVersionReview } from './PackageVersionReview';
import { PackageInstalledDetail } from './PackageInstalledDetail';
import { STATE_TEXT } from './package-text';
import './tool-packages.css';

/** The V2 tool-package page: import, review and verify, install, and manage. */
export function ToolPackagesPanel() {
  const packages = usePackages();
  const [review, setReview] = useState<PackageVersionViewV1>();
  const [diagnostics, setDiagnostics] = useState<PackageImportDiagnosticV1[]>();
  const [selected, setSelected] = useState<PackageSummaryV1['package_id']>();
  const importFrom = (command: 'packages.import_directory' | 'packages.import_archive') =>
    void packages.act(async () => {
      setDiagnostics(undefined);
      const outcome = await packages.call(command, {});
      if (outcome === undefined || outcome.type === 'cancelled') return outcome;
      if (outcome.type === 'rejected') setDiagnostics(outcome.diagnostics);
      else {
        setReview(outcome.view);
        setSelected(outcome.view.preview.version.package_id);
      }
      return outcome;
    });
  const open = (summary: PackageSummaryV1, version: PackageSummaryV1['versions'][number]) =>
    void packages.act(async () => {
      setSelected(summary.package_id);
      const view = await packages.call('packages.version.get', {
        package_id: version.version.package_id,
        package_version: version.version.package_version,
        package_fingerprint: version.version.package_fingerprint,
      });
      setReview(view);
      return view;
    });
  const current = packages.list?.packages.find((item) => item.package_id === selected);
  return (
    <section aria-label="工具包" className="settings-surface tool-packages-panel">
      <header className="tool-section-heading">
        <div>
          <h3>工具包</h3>
        </div>
        <div className="package-actions">
          <button
            disabled={packages.busy}
            onClick={() => importFrom('packages.import_directory')}
            type="button"
          >
            导入目录
          </button>
          <button
            disabled={packages.busy}
            onClick={() => importFrom('packages.import_archive')}
            type="button"
          >
            导入 ZIP
          </button>
        </div>
      </header>
      {packages.message === undefined ? null : <p role="alert">{packages.message}</p>}
      {diagnostics === undefined ? null : (
        <ul aria-label="导入问题" role="alert">
          {diagnostics.map((item, index) => (
            <li key={index}>
              {item.message}
              {item.location.type === 'manifest' && item.location.line != null
                ? `（清单第 ${item.location.line} 行${item.location.field ? `，${item.location.field}` : ''}）`
                : ''}
            </li>
          ))}
        </ul>
      )}
      {packages.list === undefined ? (
        <p role="status">正在读取工具包…</p>
      ) : packages.list.packages.length === 0 ? (
        <p>尚未导入工具包。</p>
      ) : (
        <ul className="tool-entity-list">
          {packages.list.packages.map((item) => (
            <li key={item.package_id}>
              <strong>{item.display_name}</strong> <code>{item.package_key}</code>{' '}
              {item.installed == null
                ? '未安装'
                : item.installed.trusted
                  ? item.installed.enabled
                    ? `已安装 ${item.installed.version.package_version}`
                    : `已停用 ${item.installed.version.package_version}`
                  : '已撤销信任'}
              <div className="package-actions">
                {item.versions.slice(0, 3).map((version) => (
                  <button
                    disabled={packages.busy}
                    key={version.version.package_fingerprint.hex}
                    onClick={() => open(item, version)}
                    type="button"
                  >
                    查看 {version.version.package_version}（{STATE_TEXT[version.conformance]}）
                  </button>
                ))}
                {item.installed == null ? null : (
                  <button
                    disabled={packages.busy}
                    onClick={() => {
                      setReview(undefined);
                      setSelected(item.package_id);
                    }}
                    type="button"
                  >
                    管理
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
      {review === undefined ? null : (
        <PackageVersionReview
          act={packages.act}
          busy={packages.busy}
          call={packages.call}
          key={`${review.preview.version.package_fingerprint.hex}:${review.preview.version.revision}`}
          onInstalled={() => setReview(undefined)}
          view={review}
        />
      )}
      {review === undefined && current?.installed != null ? (
        <PackageInstalledDetail
          act={packages.act}
          busy={packages.busy}
          call={packages.call}
          summary={current}
        />
      ) : null}
    </section>
  );
}
