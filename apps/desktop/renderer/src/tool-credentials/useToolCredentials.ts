import { useEffect, useRef, useState } from 'react';
import type {
  CommandInvocationId,
  IpcErrorEnvelope,
  ToolCredentialAuthorityRecordV1,
  ToolCredentialManagementTargetV1,
  ToolCredentialListResultV1,
  ToolCredentialMutationResultV1,
} from '@aura/contracts/generated';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import type { ResultEnvelope } from '../../../shared/aura-api';

export function useToolCredentials(
  target: ToolCredentialManagementTargetV1,
  onChanged: () => Promise<void>,
) {
  const [list, setList] = useState<ToolCredentialListResultV1>();
  const [message, setMessage] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [unknown, setUnknown] = useState(false);
  const receipt = useRef<CommandInvocationId | undefined>(undefined);
  const active = useRef(true);
  const working = useRef(false);
  const generation = useRef(0);
  const key = JSON.stringify(target);
  useEffect(() => {
    active.current = true;
    const id = generation.current;
    queueMicrotask(() => {
      if (active.current && id === generation.current) setList(undefined);
    });
    return () => {
      active.current = false;
      generation.current += 1;
    };
  }, [key]);
  const live = (id: number) => active.current && generation.current === id;
  async function read(id: number) {
    const result = await window.aura.v1.listToolCredentials(target);
    if (!live(id)) return;
    if (result.ok) setList(result.value);
    else setMessage('无法读取凭据状态。请刷新工具详情后重试。');
  }
  function failure(error: IpcErrorEnvelope, capturesOriginal: boolean) {
    if (isUnconfirmedCommandOutcome(error)) {
      setUnknown(true);
      const id = error.details.command_invocation_id;
      if (capturesOriginal) {
        receipt.current =
          typeof id === 'string' && /^civ_[0-7][0-9A-HJKMNP-TV-Z]{25}$/u.test(id)
            ? (id as CommandInvocationId)
            : undefined;
      }
      setMessage('结果尚未确认。请检查原操作结果，不要重复输入或提交。');
    } else if (error.category === 'conflict')
      setMessage('凭据或工具已经变化，请重新读取当前状态。');
    else if (error.code.endsWith('access_denied'))
      setMessage('Windows 拒绝访问凭据；保留原凭据，请检查系统访问条件。');
    else if (error.code.endsWith('cancelled')) setMessage('凭据操作已取消。');
    else setMessage('凭据操作未完成。请读取当前状态后处理。');
  }
  function completed(value: ToolCredentialMutationResultV1) {
    setUnknown(false);
    receipt.current = undefined;
    setMessage(
      value.disposition === 'aborted'
        ? '未提交的变更已撤回，原凭据保持。'
        : value.cleanup_pending
          ? '变更已提交，旧值清理尚未完成，可使用“恢复待完成操作”。'
          : '变更已提交。使用新凭据前，请重新读取候选并明确绑定。',
    );
  }
  async function run(action: (id: number) => Promise<void>) {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    const id = ++generation.current;
    try {
      await action(id);
    } catch {
      if (live(id)) setMessage('连接未完成；请检查当前状态。');
    } finally {
      working.current = false;
      if (active.current) setBusy(false);
    }
  }
  const refresh = () =>
    run(async (id) => {
      await read(id);
    });
  const confirm = () =>
    run(async (id) => {
      if (!receipt.current) {
        setMessage('尚未取得原操作标识。可检查待恢复列表；确认前请勿重复提交。');
        await read(id);
        return;
      }
      const result = await window.aura.v1.readToolCredentialOutcome({
        target,
        command_invocation_id: receipt.current,
      });
      if (!live(id)) return;
      if (
        result.ok &&
        result.value.result != null &&
        (result.value.state === 'committed' || result.value.state === 'aborted')
      ) {
        completed(result.value.result);
        await read(id);
        await onChanged();
      } else {
        setMessage('原操作仍待确认；未重新提交任何密钥。');
        await read(id);
      }
    });
  const mutate = (
    operation: () => Promise<ResultEnvelope<ToolCredentialMutationResultV1>>,
    resolvesOriginal = true,
  ) =>
    run(async (id) => {
      let result: ResultEnvelope<ToolCredentialMutationResultV1>;
      try {
        result = await operation();
      } catch {
        if (live(id)) {
          setUnknown(true);
          setMessage('连接中断，结果尚未确认，请检查原操作结果。');
        }
        return;
      }
      if (!live(id)) return;
      if (result.ok) {
        if (resolvesOriginal) completed(result.value);
        else setMessage('所选凭据已恢复；原操作仍待确认，请继续检查原操作结果。');
        await read(id);
        await onChanged();
      } else failure(result.error, resolvesOriginal);
    });
  const create = (name: string, bytes: Uint8Array, allowedOrigins: string[] = []) => {
    if (unknown || working.current) {
      bytes.fill(0);
      return Promise.resolve();
    }
    return mutate(async () => {
      try {
        return await window.aura.v1.createToolCredential(
          {
            target,
            display_name: name.trim(),
            scope: { scope: 'user' },
            allowed_origins: allowedOrigins,
          },
          bytes,
        );
      } finally {
        bytes.fill(0);
      }
    });
  };
  const change = (
    record: ToolCredentialAuthorityRecordV1,
    action: 'rotate' | 'clear' | 'recover',
    bytes?: Uint8Array,
  ) => {
    if ((unknown && action !== 'recover') || working.current) {
      bytes?.fill(0);
      return Promise.resolve();
    }
    const request = {
      target,
      credential_id: record.credential_id,
      expected_row_revision: record.row_revision,
    };
    return mutate(async () => {
      try {
        if (action === 'rotate' && bytes)
          return await window.aura.v1.rotateToolCredential(request, bytes);
        if (action === 'clear') return await window.aura.v1.clearToolCredential(request);
        return await window.aura.v1.recoverToolCredential(request);
      } finally {
        bytes?.fill(0);
      }
    }, !unknown);
  };
  const origins = (record: ToolCredentialAuthorityRecordV1, allowedOrigins: string[]) => {
    if (unknown || working.current) return Promise.resolve();
    return mutate(() =>
      window.aura.v1.updateToolCredentialOrigins({
        target,
        credential_id: record.credential_id,
        expected_row_revision: record.row_revision,
        allowed_origins: allowedOrigins,
      }),
    );
  };
  return { list, message, busy, unknown, refresh, confirm, create, change, origins };
}
