import { userMessage } from '../agent-session/user-message';
import { useEffect, useRef, useState } from 'react';
import type {
  TerminalOpeningPhaseV1,
  TerminalViewV1,
  UserTerminalOpenRequestV1,
} from '@aura/contracts/generated';
import type { TerminalApiV1 } from '../../../shared/terminal-api';
import { terminalApi } from './store';
import { isConfirmedOpeningCancellation } from '../../../shared/terminal-opening-validation';
interface Attempt {
  id: string;
  session: string;
  api: TerminalApiV1;
  phase?: TerminalOpeningPhaseV1;
}
interface OpeningState {
  busy: boolean;
  hint: boolean;
  phase?: TerminalOpeningPhaseV1;
  cancelling: boolean;
  message: string;
  error: string;
}
const idle: OpeningState = { busy: false, hint: false, cancelling: false, message: '', error: '' };
/** How one attempt ended; `busy` when another attempt was still running and nothing started. */
export type OpeningOutcome = 'opened' | 'cancelled' | 'failed' | 'busy';
export function useTerminalOpening(
  onOpened: (view: TerminalViewV1, api: TerminalApiV1) => Promise<void>,
) {
  const [state, setState] = useState<OpeningState>(idle);
  const current = useRef<Attempt | undefined>(undefined);
  const opened = useRef(onOpened);
  useEffect(() => {
    opened.current = onOpened;
  }, [onOpened]);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      const attempt = current.current;
      if (attempt && attempt.phase !== 'launching' && attempt.phase !== 'cancelled') {
        void attempt.api
          .cancelOpen({ contract_version: 1, opening_id: attempt.id })
          .catch(() => {});
      }
    };
  }, []);
  const open = async (request: UserTerminalOpenRequestV1): Promise<OpeningOutcome> => {
    if (current.current) return 'busy';
    const api = terminalApi();
    if (!api) {
      setState({ ...idle, error: '终端打开接口暂不可用。' });
      return 'failed';
    }
    const attempt: Attempt = { id: crypto.randomUUID(), session: request.session_id, api };
    current.current = attempt;
    setState({ ...idle, busy: true });
    const hint = setTimeout(() => {
      if (mounted.current && current.current === attempt)
        setState((value) => ({ ...value, hint: true }));
    }, 200);
    let detach = () => {};
    try {
      detach = api.onOpening((value) => {
        if (
          current.current !== attempt ||
          value.opening_id !== attempt.id ||
          value.session_id !== attempt.session
        )
          return;
        if (attempt.phase === 'launching' || attempt.phase === 'cancelled') return;
        attempt.phase = value.phase;
        if (mounted.current)
          setState((previous) => ({
            ...previous,
            phase: value.phase,
            cancelling: value.phase === 'launching' ? false : previous.cancelling,
          }));
      });
      const result = await api.beginOpen({ contract_version: 1, opening_id: attempt.id, request });
      if (result.ok) {
        await opened.current(result.value, api);
        if (mounted.current) setState(idle);
        return 'opened';
      }
      if (isConfirmedOpeningCancellation(result.error)) {
        if (mounted.current) setState({ ...idle, message: '已取消打开。' });
        return 'cancelled';
      }
      if (mounted.current)
        setState({ ...idle, error: userMessage(result.error, '终端没有打开，请重试。') });
      return 'failed';
    } catch {
      if (mounted.current)
        setState({ ...idle, error: '终端打开结果尚未确认，请检查 Core 状态；不要自动重试。' });
      return 'failed';
    } finally {
      clearTimeout(hint);
      detach();
      if (current.current === attempt) current.current = undefined;
    }
  };
  const cancel = async () => {
    const attempt = current.current;
    if (
      !attempt ||
      attempt.phase === 'launching' ||
      attempt.phase === 'cancelled' ||
      state.cancelling
    )
      return;
    setState((value) => ({ ...value, cancelling: true, error: '' }));
    try {
      const result = await attempt.api.cancelOpen({ contract_version: 1, opening_id: attempt.id });
      if (!mounted.current || current.current !== attempt) return;
      if (result.ok && result.value === 'accepted') return;
      if (result.ok && result.value === 'too_late_to_cancel') {
        attempt.phase = 'launching';
        setState((value) => ({ ...value, phase: 'launching', cancelling: false }));
      } else
        setState((value) => ({
          ...value,
          error: result.ok
            ? '取消尚未确认，请等待本次打开结果。'
            : userMessage(result.error, '取消没有完成，请等待本次打开结果。'),
        }));
    } catch {
      if (mounted.current && current.current === attempt)
        setState((value) => ({ ...value, error: '取消尚未确认，请等待本次打开结果。' }));
    }
  };
  const message =
    state.busy && state.hint
      ? state.cancelling
        ? '正在取消打开…'
        : state.phase === 'waiting_runtime'
          ? '正在校验 PowerShell 运行环境…'
          : state.phase === 'waiting_launch'
            ? '正在等待启动终端…'
            : '正在打开终端…'
      : state.message;
  return {
    open,
    cancel,
    busy: state.busy,
    error: state.error,
    message,
    canCancel: state.busy && state.phase !== 'launching' && state.phase !== 'cancelled',
    cancelling: state.cancelling,
  };
}
