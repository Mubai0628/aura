import { useEffect, useMemo, useState } from 'react';

import { AppIcon } from '../app-icons';
import type {
  CanonicalToolPackageToolV1,
  PackageConformancePreparationV1,
  PackageConformanceViewV1,
  PackageToolSettingsV1,
  PackageVersionViewV1,
} from '@aura/contracts/generated';
import type { PackageCall } from './usePackages';
import {
  CAPABILITY_TEXT,
  REASON_TEXT,
  STATE_TEXT,
  directoryText,
  fingerprintPrefix,
  httpTarget,
} from './package-text';
import { SettingsForm } from './PackageSettingsForm';
import { settingsNeeded } from './package-settings';

interface Props {
  view: PackageVersionViewV1;
  call: PackageCall;
  act: <T>(action: () => Promise<T | undefined>) => Promise<T | undefined>;
  busy: boolean;
  onInstalled: () => void;
}

/** One imported version: what it asks for, its verification, and the install approval. */
export function PackageVersionReview({ view, call, act, busy, onInstalled }: Props) {
  const preview = view.preview;
  const manifest = preview.content.manifest;
  const [conformance, setConformance] = useState<PackageConformanceViewV1>(view.conformance);
  const [prepared, setPrepared] = useState<PackageConformancePreparationV1>();
  const [settings, setSettings] = useState<PackageToolSettingsV1[]>(view.settings);
  const [settingsValid, setSettingsValid] = useState(true);
  const reference = useMemo(
    () => ({
      package_id: preview.version.package_id,
      package_version: preview.version.package_version,
      package_fingerprint: preview.version.package_fingerprint,
    }),
    [preview.version],
  );
  const running = conformance.state === 'running';
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => {
      void call('packages.conformance.get', reference).then((next) => {
        if (next !== undefined) setConformance(next);
      });
    }, 1000);
    return () => clearInterval(timer);
  }, [running, call, reference]);
  const unsupported = preview.runtimes.some(
    (runtime) => runtime.support === 'program_credential_unsupported',
  );
  const programs = preview.runtimes.some((runtime) => runtime.execution_location === 'user_job');
  const prepare = () =>
    void act(async () => {
      const value = await call('packages.conformance.prepare', {
        version: conformance.version,
        settings,
      });
      setPrepared(value);
      return value;
    });
  const start = () =>
    void act(async () => {
      if (prepared === undefined) return undefined;
      setPrepared(undefined);
      const value = await call('packages.conformance.start', {
        preparation_id: prepared.preparation_id,
        preview_hash: prepared.preview_hash,
      });
      if (value !== undefined) setConformance(value);
      return value;
    });
  const cancel = () =>
    void act(async () => {
      if (conformance.run_id == null) return undefined;
      const value = await call('packages.conformance.cancel', {
        version: reference,
        run_id: conformance.run_id,
      });
      if (value !== undefined) setConformance(value);
      return value;
    });
  const install = () =>
    void act(async () => {
      if (conformance.report_hash == null) return undefined;
      const value = await call('packages.install', {
        version: conformance.version,
        expected_package_revision: view.package_revision,
        report_hash: conformance.report_hash,
      });
      if (value !== undefined) onInstalled();
      return value;
    });
  return (
    <section aria-label="工具包版本" className="package-review">
      <header className="tool-section-heading">
        <div>
          <h4>
            {manifest.ui.display_name} <small>{manifest.package_version}</small>
          </h4>
          <p>
            <code>{manifest.package_key}</code> · 指纹{' '}
            <code>{fingerprintPrefix(preview.version.package_fingerprint.hex)}</code>
          </p>
        </div>
        <span className="package-state">{STATE_TEXT[conformance.state]}</span>
      </header>
      {programs ? (
        <div className="truth-notice" role="note">
          <AppIcon name="warning" />
          <p>
            这个包的代码会以你的 Windows
            用户身份运行，与终端相同，不是沙箱。只安装你信任的作者提供的包。
          </p>
        </div>
      ) : null}
      {unsupported ? (
        <p role="alert">
          这个包有运行程序的工具声明了凭据。运行程序的工具不能使用 Aura 保存的凭据，因此不能安装。
        </p>
      ) : null}
      <ul className="package-tools">
        {manifest.tools.map((tool) => (
          <ToolFacts
            key={tool.tool_key}
            tool={tool}
            directory={preview.runtimes.find((runtime) => runtime.tool_key === tool.tool_key)}
          />
        ))}
      </ul>
      {settingsNeeded(manifest.tools) ? (
        <SettingsForm
          disabled={busy || running || prepared !== undefined}
          onChange={(next, valid) => {
            setSettings(next);
            setSettingsValid(valid);
          }}
          settings={settings}
          tools={manifest.tools}
        />
      ) : null}
      <ConformanceReport conformance={conformance} />
      {prepared === undefined ? null : (
        <div className="package-approval" role="dialog" aria-label="批准运行验证">
          <p>
            验证会按包里声明的用例运行一次它的工具。运行程序的工具以你的身份在临时目录中运行；HTTP
            工具不会连接真实服务，由 Aura 在本机模拟回应。
          </p>
          <ul>
            {prepared.preview.runtimes.map((runtime) => (
              <li key={runtime.tool_key}>
                {runtime.tool_key}：{directoryText(runtime.working_directory)}
              </li>
            ))}
          </ul>
          <button disabled={busy} onClick={start} type="button">
            批准并运行验证
          </button>
          <button disabled={busy} onClick={() => setPrepared(undefined)} type="button">
            取消
          </button>
        </div>
      )}
      <div className="package-actions">
        {running ? (
          <button disabled={busy} onClick={cancel} type="button">
            取消验证
          </button>
        ) : (
          <button
            disabled={busy || unsupported || !settingsValid || prepared !== undefined}
            onClick={prepare}
            type="button"
          >
            {conformance.state === 'not_run' ? '准备验证' : '重新验证'}
          </button>
        )}
        {conformance.state === 'succeeded' && conformance.report_hash != null ? (
          <button disabled={busy || unsupported} onClick={install} type="button">
            安装：信任这个版本并启用
          </button>
        ) : null}
      </div>
    </section>
  );
}

function ToolFacts({
  tool,
  directory,
}: {
  tool: CanonicalToolPackageToolV1;
  directory: PackageVersionViewV1['preview']['runtimes'][number] | undefined;
}) {
  const target = httpTarget(tool);
  const credential =
    tool.runtime.type === 'declarative_http' ? tool.runtime.request.credential_slot : undefined;
  const networked = tool.capability_requirements.some((capability) =>
    ['public_network_read', 'private_network_access', 'data_egress'].includes(capability),
  );
  return (
    <li>
      <strong>{tool.ui.display_name}</strong> <code>{tool.model.canonical_name}</code>
      <p>{tool.model.description}</p>
      <p>
        {target === undefined
          ? `运行方式：以你的身份运行程序（${directory ? directoryText(directory.working_directory) : ''}）`
          : `运行方式：由 Aura 代发请求，只能访问 ${target}`}
      </p>
      {target === undefined && networked ? (
        <p>联网：由程序自行联网，Aura 不限定地址，按终端的联网规则询问。</p>
      ) : null}
      {credential ? <p>凭据：需要绑定一个允许网站包含该地址的 Aura 工具凭据。</p> : null}
      <p>
        声明的能力：
        {tool.capability_requirements.map((capability) => CAPABILITY_TEXT[capability]).join('、') ||
          '无'}
      </p>
    </li>
  );
}

function ConformanceReport({ conformance }: { conformance: PackageConformanceViewV1 }) {
  if (conformance.report == null) return null;
  return (
    <table className="package-report">
      <thead>
        <tr>
          <th>工具</th>
          <th>用例</th>
          <th>结果</th>
        </tr>
      </thead>
      <tbody>
        {conformance.report.cases.map((item) => (
          <tr key={`${item.tool_key}:${item.case_name}`}>
            <td>{item.tool_key}</td>
            <td>{item.case_name}</td>
            <td>{REASON_TEXT[item.reason]}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
