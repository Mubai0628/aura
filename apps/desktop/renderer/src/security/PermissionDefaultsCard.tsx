import { userMessage } from '../agent-session/user-message';
import type { PermissionDefaults, PermissionMode } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { ConfirmationDialog } from '../ConfirmationDialog';
import type { NotificationInput } from '../notification-center-store';
import { FULL_ACCESS_DELETES } from './full-access-notice';

const MODES: ReadonlyArray<{ value: PermissionMode; label: string; description: string }> = [
  { value: 'read_only', label: '只读', description: '读取与分析文件，保持内容不变。' },
  {
    value: 'workspace_read_write',
    label: '工作区读写',
    description: '允许修改工作区文件、运行命令、公开搜索与网页读取。',
  },
  {
    value: 'full_access',
    label: '完全访问',
    description:
      '删除、下载与写工作区外的命令也不再询问，还允许工具联网与上传文件，具体操作仍受授权约束。',
  },
];

export function PermissionDefaultsCard({
  onNotify,
}: {
  onNotify: (notification: NotificationInput) => void;
}) {
  const [snapshot, setSnapshot] = useState<PermissionDefaults>();
  const [selected, setSelected] = useState<PermissionMode>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    void window.aura.v1
      .getPermissionDefaults()
      .then((result) => {
        if (!mounted.current) return;
        if (result.ok) {
          setSnapshot(result.value);
          setSelected(result.value.mode);
        } else setError(userMessage(result.error, '默认权限读取失败，请重试。'));
      })
      .catch((reason: unknown) => {
        if (mounted.current) setError(userMessage(reason, '读取默认权限失败。'));
      });
    return () => {
      mounted.current = false;
    };
  }, []);

  const save = async () => {
    if (snapshot === undefined || selected === undefined || busy) return;
    setBusy(true);
    setError(undefined);
    try {
      const result = await window.aura.v1.setPermissionDefaults({
        expected_revision: snapshot.revision,
        mode: selected,
      });
      if (!mounted.current) return;
      if (!result.ok) {
        setError(userMessage(result.error, '默认权限没有保存，请刷新后重试。'));
        const latest = await window.aura.v1.getPermissionDefaults();
        if (mounted.current && latest.ok) setSnapshot(latest.value);
        return;
      }
      setSnapshot(result.value);
      setSelected(result.value.mode);
      setConfirm(false);
      onNotify({
        title: '默认权限已保存',
        message: '只影响以后新建的会话。',
        tone: 'success',
        dedupeKey: `permission-defaults:${result.value.revision}`,
      });
    } catch (reason) {
      if (mounted.current) setError(userMessage(reason, '保存失败，请重新读取后确认。'));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  return (
    <section
      className="settings-surface permission-defaults-surface"
      aria-labelledby="permission-defaults-heading"
    >
      <header>
        <h3 id="permission-defaults-heading">会话默认权限</h3>
      </header>
      {snapshot === undefined && error === undefined ? (
        <p role="status">正在读取默认权限…</p>
      ) : null}
      <div className="default-permission-options" role="radiogroup" aria-label="新会话默认权限">
        {MODES.map((mode) => (
          <label key={mode.value} className={selected === mode.value ? 'is-selected' : undefined}>
            <input
              type="radio"
              name="permission-default"
              value={mode.value}
              checked={selected === mode.value}
              disabled={snapshot === undefined || busy}
              onChange={() => setSelected(mode.value)}
            />
            <span>
              <strong>
                {mode.label}
                {snapshot?.mode === mode.value ? (
                  <span className="permission-default-current">当前默认</span>
                ) : null}
              </strong>
              <small>{mode.description}</small>
            </span>
          </label>
        ))}
      </div>
      {error === undefined ? null : (
        <p role="alert" className="settings-error">
          {error}
        </p>
      )}
      <footer>
        <button
          className="primary-button"
          type="button"
          disabled={snapshot === undefined || busy || selected === snapshot.mode}
          onClick={() => (selected === 'full_access' ? setConfirm(true) : void save())}
        >
          {busy ? '正在保存…' : '保存默认权限'}
        </button>
      </footer>
      {confirm ? (
        <ConfirmationDialog
          busy={busy}
          title="将完全访问设为新会话默认值"
          titleId="default-permission-confirmation"
          description={`以后新建的会话将允许工具联网和上传文件。已有会话不会改变，也不会自动授予管理员权限。${FULL_ACCESS_DELETES}`}
          confirmLabel="保存默认值"
          onCancel={() => setConfirm(false)}
          onConfirm={() => void save()}
        />
      ) : null}
    </section>
  );
}
