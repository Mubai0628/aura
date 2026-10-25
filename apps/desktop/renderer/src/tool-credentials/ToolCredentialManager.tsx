import type {
  ToolCredentialManagementTargetV1,
  ToolCredentialStateV1,
} from '@aura/contracts/generated';
import { useId, useState } from 'react';
import { useToolCredentials } from './useToolCredentials';
import { allowedOrigins } from './allowed-origins';
import './tool-credentials.css';

const STATES: Record<ToolCredentialStateV1, string> = {
  configured: '已配置',
  suspended: '已暂停',
  revoked: '已清除',
  recovery_required: '需要恢复',
};

export function ToolCredentialManager({
  target,
  disabled,
  onChanged,
}: {
  target: ToolCredentialManagementTargetV1;
  disabled: boolean;
  onChanged: () => Promise<void>;
}) {
  const control = useToolCredentials(target, onChanged);
  const id = useId();
  const [name, setName] = useState('');
  const [secret, setSecret] = useState('');
  const [selected, setSelected] = useState('');
  const [origins, setOrigins] = useState('');
  const [originError, setOriginError] = useState<string>();
  const [action, setAction] = useState<'create' | 'rotate' | 'clear' | 'recover' | 'origins'>(
    'create',
  );
  const [confirmClear, setConfirmClear] = useState(false);
  const record = control.list?.credentials.find((r) => r.credential_id === selected);
  const pending =
    record !== undefined && control.list?.pending_credential_ids.includes(record.credential_id);
  const locked = disabled || control.busy;
  const cannotSubmit =
    locked ||
    (control.unknown && action !== 'recover') ||
    (action !== 'create' && record === undefined) ||
    (action === 'clear' && !confirmClear) ||
    ((action === 'create' || action === 'rotate') && !secret.trim()) ||
    (action === 'create' && !name.trim()) ||
    (pending === true && action !== 'recover');
  return (
    <section
      aria-label="管理工具凭据"
      aria-busy={control.busy}
      className="tool-credential-binding-control"
    >
      <h4>管理工具凭据</h4>
      <p>
        新凭据属于当前用户，只允许此工具在已声明的位置使用。密钥不显示在详情中；创建或轮换后需重新选择并绑定。
      </p>
      <button disabled={locked} onClick={() => void control.refresh()} type="button">
        读取凭据管理状态
      </button>
      {control.message ? <p role="status">{control.message}</p> : null}
      {control.unknown ? (
        <button disabled={locked} onClick={() => void control.confirm()} type="button">
          检查原操作结果
        </button>
      ) : null}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (cannotSubmit) return;
          let sites: string[];
          try {
            sites = allowedOrigins(origins);
            setOriginError(undefined);
          } catch (error) {
            setOriginError(error instanceof Error ? error.message : '允许网站格式无效。');
            return;
          }
          const bytes = new TextEncoder().encode(secret);
          setSecret('');
          setConfirmClear(false);
          if (action === 'create') void control.create(name, bytes, sites);
          else if (action === 'origins' && record) {
            bytes.fill(0);
            void control.origins(record, sites);
          } else if (record) {
            if (action !== 'rotate') bytes.fill(0);
            if (action !== 'origins')
              void control.change(record, action, action === 'rotate' ? bytes : undefined);
          } else bytes.fill(0);
        }}
      >
        <label htmlFor={`${id}-action`}>操作</label>
        <select
          disabled={locked}
          id={`${id}-action`}
          value={action}
          onChange={(event) => {
            setAction(event.target.value as typeof action);
            setOrigins(
              event.target.value === 'origins' ? (record?.allowed_origins ?? []).join('\n') : '',
            );
            setOriginError(undefined);
            setSecret('');
            setConfirmClear(false);
          }}
        >
          <option value="create">创建新凭据</option>
          <option value="rotate">轮换密钥</option>
          <option value="clear">清除凭据密钥</option>
          <option value="recover">恢复待完成操作</option>
          <option value="origins">修改允许网站</option>
        </select>
        {action === 'create' ? (
          <label htmlFor={`${id}-name`}>
            名称
            <input
              disabled={locked}
              id={`${id}-name`}
              maxLength={120}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
        ) : (
          <>
            <label htmlFor={`${id}-record`}>凭据</label>
            <select
              disabled={locked}
              id={`${id}-record`}
              value={selected}
              onChange={(e) => {
                setSelected(e.target.value);
                setOrigins(
                  (
                    control.list?.credentials.find(
                      (value) => value.credential_id === e.target.value,
                    )?.allowed_origins ?? []
                  ).join('\n'),
                );
                setOriginError(undefined);
                setConfirmClear(false);
              }}
            >
              <option value="">先读取状态，再选择凭据</option>
              {control.list?.credentials.map((r) => (
                <option key={r.credential_id} value={r.credential_id}>
                  {r.display_name} · {STATES[r.state]}
                  {control.list?.pending_credential_ids.includes(r.credential_id)
                    ? ' · 待恢复'
                    : ''}
                </option>
              ))}
            </select>
          </>
        )}
        {action === 'create' || action === 'rotate' ? (
          <label htmlFor={`${id}-secret`}>
            密钥
            <input
              autoComplete="off"
              disabled={locked || control.unknown}
              id={`${id}-secret`}
              type="password"
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
            />
          </label>
        ) : null}
        {action === 'clear' ? (
          <label>
            <input
              type="checkbox"
              checked={confirmClear}
              disabled={locked}
              onChange={(e) => setConfirmClear(e.target.checked)}
            />
            确认清除“{record?.display_name ?? '所选凭据'}
            ”的密钥。使用它的绑定将不可用；其他凭据保持。
          </label>
        ) : null}
        {record ? (
          <p>
            当前允许网站：
            {record.allowed_origins?.join('、') || '未配置（不能用于模型指定网址的 HTTP 请求）'}
          </p>
        ) : null}
        {action === 'create' || action === 'origins' ? (
          <label htmlFor={`${id}-origins`}>
            允许网站（每行一个来源）
            <textarea
              id={`${id}-origins`}
              disabled={locked}
              value={origins}
              onChange={(event) => setOrigins(event.target.value)}
              placeholder="https://api.example.com"
            />
            <span>
              凭据只发往这些来源；端口和协议必须一致。留空时不能用于
              http.request。修改网站不更换密钥。
            </span>
          </label>
        ) : null}
        {originError ? <p role="alert">{originError}</p> : null}
        <button disabled={cannotSubmit} type="submit">
          {action === 'create'
            ? '创建凭据'
            : action === 'rotate'
              ? '确认轮换密钥'
              : action === 'clear'
                ? '确认清除密钥'
                : action === 'origins'
                  ? '保存允许网站'
                  : '恢复所选凭据'}
        </button>
      </form>
    </section>
  );
}
