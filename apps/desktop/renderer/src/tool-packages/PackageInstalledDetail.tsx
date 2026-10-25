import { useState } from 'react';
import type {
  PackageSummaryV1,
  PackageToolSummaryV1,
  ToolCredentialAuthorityRecordV1,
} from '@aura/contracts/generated';
import { ToolCredentialManager } from '../tool-credentials/ToolCredentialManager';
import type { PackageCall } from './usePackages';
import { fingerprintPrefix } from './package-text';

interface Props {
  summary: PackageSummaryV1;
  call: PackageCall;
  act: <T>(action: () => Promise<T | undefined>) => Promise<T | undefined>;
  busy: boolean;
}

type Change = 'disable' | 'enable' | 'revoke' | 'uninstall';

const IMPACT: Record<Change, string> = {
  disable:
    '停用后，新的会话轮次看不到这个包的工具，已发出但未开始的调用会被拒绝；正在运行的调用允许跑完。各会话对这个包的“本会话允许”同时撤销，重新启用后会再次询问。',
  enable: '启用后，模型在新的会话轮次可以使用这个包的工具。',
  revoke:
    '撤销信任后这个包的工具不能再运行，也会被停用，各会话对它的“本会话允许”同时撤销；要再次使用需重新验证并安装。正在运行的调用允许跑完。',
  uninstall:
    '卸载会移除这个包的所有版本、设置和凭据绑定，并撤销各会话对它的“本会话允许”；没有运行中的调用再使用的内容会立即删除。已保存的凭据本身不会删除。',
};

/** An installed package: state, the explicit lifecycle changes and HTTP credentials. */
export function PackageInstalledDetail({ summary, call, act, busy }: Props) {
  const installed = summary.installed;
  const [confirm, setConfirm] = useState<Change>();
  if (installed == null) return null;
  const apply = (change: Change) =>
    void act(async () => {
      setConfirm(undefined);
      const base = {
        package_id: summary.package_id,
        expected_package_revision: summary.revision,
      };
      switch (change) {
        case 'disable':
        case 'enable':
          return call('packages.set_enabled', { ...base, enabled: change === 'enable' });
        case 'revoke':
          return call('packages.revoke_trust', base);
        case 'uninstall':
          return call('packages.uninstall', base);
      }
    });
  return (
    <section aria-label="已安装的工具包" className="package-installed">
      <p>
        已安装 {installed.version.package_version} · 指纹{' '}
        <code>{fingerprintPrefix(installed.version.package_fingerprint.hex)}</code> ·{' '}
        {installed.trusted ? (installed.enabled ? '已启用' : '已停用') : '已撤销信任'}
      </p>
      <div className="package-actions">
        {installed.trusted ? (
          <button
            disabled={busy}
            onClick={() => setConfirm(installed.enabled ? 'disable' : 'enable')}
            type="button"
          >
            {installed.enabled ? '停用' : '启用'}
          </button>
        ) : null}
        {installed.trusted ? (
          <button disabled={busy} onClick={() => setConfirm('revoke')} type="button">
            撤销信任
          </button>
        ) : null}
        <button disabled={busy} onClick={() => setConfirm('uninstall')} type="button">
          卸载
        </button>
      </div>
      {confirm === undefined ? null : (
        <div className="package-approval" role="dialog" aria-label="确认工具包更改">
          <p>{IMPACT[confirm]}</p>
          <button disabled={busy} onClick={() => apply(confirm)} type="button">
            确认
          </button>
          <button disabled={busy} onClick={() => setConfirm(undefined)} type="button">
            取消
          </button>
        </div>
      )}
      <ul className="package-tools">
        {installed.tools.map((tool) => (
          <li key={tool.tool_key}>
            <strong>{tool.display_name}</strong> <code>{tool.canonical_name}</code>
            {tool.credential == null || !installed.trusted ? null : (
              <PackageCredential
                act={act}
                busy={busy}
                call={call}
                packageId={summary.package_id}
                tool={tool}
              />
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function PackageCredential({
  tool,
  packageId,
  call,
  act,
  busy,
}: {
  tool: PackageToolSummaryV1;
  packageId: PackageSummaryV1['package_id'];
  call: PackageCall;
  act: Props['act'];
  busy: boolean;
}) {
  const slot = tool.credential!;
  const [choices, setChoices] = useState<ToolCredentialAuthorityRecordV1[]>();
  const [choice, setChoice] = useState('');
  const load = async () => {
    const result = await window.aura.v1.listToolCredentials(slot.target);
    if (!result.ok) return;
    setChoices(
      result.value.credentials.filter(
        (record) =>
          record.state === 'configured' && (record.allowed_origins ?? []).includes(slot.origin),
      ),
    );
  };
  const bind = () =>
    void act(async () => {
      const selected = choices?.find((item) => item.credential_id === choice);
      if (selected === undefined) return undefined;
      return call('packages.credential.bind', {
        package_id: packageId,
        tool_key: tool.tool_key,
        slot_key: slot.slot_key,
        credential_id: selected.credential_id,
        credential_revision: selected.credential_revision,
        expected_binding_revision: slot.binding?.revision ?? null,
      });
    });
  const clear = () =>
    void act(async () =>
      slot.binding == null
        ? undefined
        : call('packages.credential.clear', {
            package_id: packageId,
            tool_key: tool.tool_key,
            slot_key: slot.slot_key,
            expected_binding_revision: slot.binding.revision,
          }),
    );
  return (
    <div className="package-credential">
      <p>
        这个工具需要凭据，只会发送到 <code>{slot.origin}</code>。
        {slot.binding == null ? '尚未绑定。' : '已绑定一个凭据。'}
      </p>
      <ToolCredentialManager disabled={busy} onChanged={load} target={slot.target} />
      <button disabled={busy} onClick={() => void load()} type="button">
        读取可绑定的凭据
      </button>
      {choices === undefined ? null : choices.length === 0 ? (
        <p>没有允许网站包含该地址的已配置凭据。请在上面新建，并在允许网站中填写该地址。</p>
      ) : (
        <label>
          选择凭据
          <select onChange={(event) => setChoice(event.target.value)} value={choice}>
            <option value="">请选择</option>
            {choices.map((item) => (
              <option key={item.credential_id} value={item.credential_id}>
                {item.display_name}
              </option>
            ))}
          </select>
        </label>
      )}
      <button disabled={busy || choice === ''} onClick={bind} type="button">
        绑定
      </button>
      {slot.binding == null ? null : (
        <button disabled={busy} onClick={clear} type="button">
          解除绑定
        </button>
      )}
    </div>
  );
}
