import type {
  BundleRecoveryPlan,
  ConfigRecoveryAction,
  RecoveryNotice,
  RecoveryReport,
} from '@aura/contracts/generated';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import { useCallback, useEffect, useRef, useState } from 'react';

export function useConfigRecovery(coreReady: boolean) {
  const [notice, setNotice] = useState<RecoveryNotice>();
  const [plan, setPlan] = useState<BundleRecoveryPlan>();
  const [report, setReport] = useState<RecoveryReport>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string>();
  const [unknown, setUnknown] = useState(false);
  const operation = useRef(0);
  const active = useRef(false);
  const pending = useRef(false);

  const readNotice = useCallback(async (id: number) => {
    const result = await window.aura.v1.getConfigRecoveryNotice();
    if (!active.current || id !== operation.current) return false;
    if (result.ok) {
      setNotice(result.value);
      return !result.value.recovery_in_progress;
    }
    setMessage('暂时无法查看恢复状态。请检查 Core 状态后重试。');
    return false;
  }, []);

  useEffect(() => {
    active.current = true;
    const id = ++operation.current;
    const interrupted = pending.current;
    pending.current = false;
    queueMicrotask(() => {
      if (!active.current || id !== operation.current) return;
      setPlan(undefined);
      if (interrupted) {
        setUnknown(true);
        setMessage('Core 状态已变化，先检查当前状态再进行恢复。');
      }
      setBusy(false);
    });
    if (coreReady) {
      void readNotice(id).catch(() => {
        if (active.current && id === operation.current) setMessage('暂时无法查看恢复状态。');
      });
    }
    return () => {
      active.current = false;
      operation.current += 1;
    };
  }, [coreReady, readNotice]);

  const preview = async () => {
    if (pending.current || !coreReady) return;
    pending.current = true;
    const id = ++operation.current;
    setBusy(true);
    setMessage(undefined);
    setPlan(undefined);
    try {
      // Core resolves the actual affected set, including an unjudged bundle's complete scope.
      const result = await window.aura.v1.planConfigRecovery({ domains: [] });
      if (!active.current || id !== operation.current) return;
      if (result.ok) {
        setPlan(result.value);
        setUnknown(false);
      } else setMessage('无法生成恢复预览。未执行恢复，请先检查当前状态。');
    } catch {
      if (active.current && id === operation.current)
        setMessage('读取预览失败。未执行恢复，请重试。');
    } finally {
      if (id === operation.current) pending.current = false;
      if (active.current && id === operation.current) setBusy(false);
    }
  };

  const apply = async (action: ConfigRecoveryAction) => {
    if (
      pending.current ||
      plan === undefined ||
      unknown ||
      !plan.options.some((o) => o.action === action && o.available)
    )
      return;
    pending.current = true;
    const id = ++operation.current;
    setBusy(true);
    setMessage(undefined);
    // Consume the visible plan once. A failure never silently re-plans and re-sends.
    setPlan(undefined);
    try {
      const single = plan.domains.length === 1 ? plan.domains[0] : undefined;
      const result = await window.aura.v1.applyConfigRecovery(
        single === undefined
          ? { action, domains: plan.modules, preview_id: plan.preview_id }
          : { action, domain: single.module, domains: [], preview_id: single.preview_id },
      );
      if (!active.current || id !== operation.current) return;
      if (result.ok) {
        setReport(result.value);
        setUnknown(false);
        await readNotice(id);
      } else if (result.error.code.endsWith('preview_stale')) {
        setMessage('设置已经变化。本次未覆盖新设置，请重新查看预览后确认。');
      } else if (
        isUnconfirmedCommandOutcome(result.error) ||
        result.error.code.includes('boundary_unknown')
      ) {
        setUnknown(true);
        setMessage('恢复结果尚未确认。请先检查当前状态，不要重复提交相同操作。');
      } else setMessage('恢复未完成。请检查当前状态，重新查看可用选项。');
    } catch {
      if (active.current && id === operation.current) {
        setUnknown(true);
        setMessage('连接中断，恢复结果尚未确认。请先检查当前状态。');
      }
    } finally {
      if (id === operation.current) pending.current = false;
      if (active.current && id === operation.current) setBusy(false);
    }
  };

  const refresh = async () => {
    if (pending.current || !coreReady) return;
    pending.current = true;
    setBusy(true);
    const id = ++operation.current;
    setPlan(undefined);
    try {
      const obtained = await readNotice(id);
      // Checking current state is read-only. A fresh preview and confirmation is still required.
      if (obtained && active.current && id === operation.current) setUnknown(false);
    } catch {
      if (active.current && id === operation.current)
        setMessage('状态暂时无法确认，请稍后再检查。');
    } finally {
      if (id === operation.current) pending.current = false;
      if (active.current && id === operation.current) setBusy(false);
    }
  };
  return { notice, plan, report, busy, message, unknown, preview, apply, refresh };
}
