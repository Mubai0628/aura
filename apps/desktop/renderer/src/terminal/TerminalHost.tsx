import { userMessage } from '../agent-session/user-message';
import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import {
  addTerminal,
  startTerminalStore,
  subscribeTerminals,
  terminalApi,
  terminalSnapshot,
} from './store';
import { TerminalScreen } from './TerminalScreen';
import { useTerminalOpening } from './useTerminalOpening';
import { useUiSettings } from '../ui-settings-store';
import { toolTabs, type ToolTab } from '../right-workspace/tool-tabs';
import './terminal.css';

/**
 * The conversation's terminal tabs. A new tab opens the default terminal
 * chosen in 设置 → 环境 at once — there is no shell to pick here — one tab at a time. Every screen
 * stays mounted, hidden while its tab is not shown, so a terminal keeps its scrollback across tabs
 * and conversations. The model's terminals are listed but shown only when the user opens their tab.
 */
export function TerminalHost({
  sessionId,
  tabs,
  activeTab,
  visible,
}: {
  sessionId?: string | undefined;
  tabs: ToolTab[];
  activeTab?: string | undefined;
  visible: boolean;
}) {
  const records = useSyncExternalStore(subscribeTerminals, terminalSnapshot);
  const ui = useUiSettings();
  /** Tabs whose terminal did not open, and why ('' while the hook's own message explains it). */
  const [unopened, setUnopened] = useState<Record<string, string>>({});
  const [listed, setListed] = useState<string>();
  /** Why this conversation's terminals could not be listed; another conversation's reason is old. */
  const [listError, setListError] = useState<{ session: string; message: string }>();
  const target = useRef<{ session: string; tab: string } | undefined>(undefined);
  const opening = useTerminalOpening(async (view, api) => {
    const tab = target.current?.tab;
    // The tab was closed while its terminal was starting: the terminal goes with it.
    if (!tab || !toolTabs.get(view.session_id).tabs.some((item) => item.id === tab)) {
      await api.close({ session_id: view.session_id, terminal_id: view.terminal_id });
      return;
    }
    await addTerminal(view, api);
    toolTabs.bindTerminal(view.session_id, tab, view.terminal_id);
  });

  useEffect(() => {
    startTerminalStore();
    if (!sessionId) return;
    let live = true;
    void (async () => {
      const api = terminalApi();
      if (!api) return;
      let cursor: string | null | undefined;
      do {
        const result = await api.list({ session_id: sessionId, cursor: cursor ?? null });
        if (!live) return;
        if (!result.ok) {
          setListError({
            session: sessionId,
            message: userMessage(result.error, '终端列表读取失败，请重试。'),
          });
          return;
        }
        for (const view of result.value.terminals) {
          await addTerminal(view, api);
          if (view.owner === 'user' && !view.end)
            toolTabs.adoptTerminal(sessionId, view.terminal_id);
        }
        cursor = result.value.next_cursor;
      } while (cursor && live);
      if (!live) return;
      setListed(sessionId);
      setListError((value) => (value?.session === sessionId ? undefined : value));
    })();
    return () => {
      live = false;
    };
  }, [sessionId]);

  const pending = tabs.find(
    (tab) => tab.kind === 'terminal' && !tab.terminalId && unopened[tab.id] === undefined,
  )?.id;
  const busy = opening.busy;
  const loading = ui.loading;
  const shell = ui.settings.default_terminal;
  const [startingTab, setStartingTab] = useState<string>();
  const latest = useRef({ open: opening.open, cancel: opening.cancel });
  useEffect(() => {
    latest.current = { open: opening.open, cancel: opening.cancel };
  });
  useEffect(() => {
    if (!sessionId || !pending || busy || loading) return;
    // Started on a timer, so a mount React repeats in development starts one opening, not two.
    const timer = setTimeout(() => {
      target.current = { session: sessionId, tab: pending };
      setStartingTab(pending);
      void latest.current
        .open({ session_id: sessionId, shell, columns: 80, rows: 24 })
        .then((outcome) => {
          setStartingTab((value) => (value === pending ? undefined : value));
          if (outcome === 'cancelled')
            setUnopened((value) => ({ ...value, [pending]: '已取消打开。' }));
          else if (outcome === 'failed') setUnopened((value) => ({ ...value, [pending]: '' }));
        });
    }, 0);
    return () => clearTimeout(timer);
  }, [sessionId, pending, busy, loading, shell]);

  // A tab closed while its terminal is starting cancels the start.
  useEffect(() => {
    if (busy && startingTab && !tabs.some((tab) => tab.id === startingTab))
      void latest.current.cancel();
  }, [tabs, busy, startingTab]);

  const tab = tabs.find((item) => item.id === activeTab && item.kind === 'terminal');
  const shown = tab?.terminalId;
  const starting = tab !== undefined && startingTab === tab.id && busy;
  const reason = tab && !tab.terminalId ? unopened[tab.id] : undefined;
  return (
    <section className="terminal-panel" aria-label="终端" hidden={!visible || !tab}>
      {tab && !tab.terminalId && reason === undefined ? (
        <p className="terminal-status" role="status">
          <span>{starting ? opening.message || '正在打开终端…' : '正在打开终端…'}</span>
          {starting && opening.canCancel ? (
            <button
              type="button"
              aria-label="取消打开"
              disabled={opening.cancelling}
              onClick={() => void opening.cancel()}
            >
              取消
            </button>
          ) : null}
        </p>
      ) : null}
      {tab && reason !== undefined ? (
        <div className="terminal-status" role={reason ? 'status' : 'alert'}>
          <span>{reason || opening.error || '终端没有打开，请重试。'}</span>
          <button
            type="button"
            onClick={() =>
              setUnopened((value) => {
                const next = { ...value };
                delete next[tab.id];
                return next;
              })
            }
          >
            重新打开
          </button>
        </div>
      ) : null}
      {tab?.terminalId && !records.some((record) => record.view.terminal_id === shown) ? (
        <p className="terminal-status" role="status">
          {listed === sessionId ? '这个终端已不可用。' : '正在连接终端…'}
        </p>
      ) : null}
      {listError && listError.session === sessionId ? (
        <p className="terminal-error" role="alert">
          {listError.message}
        </p>
      ) : null}
      {records.map((record) => (
        <TerminalScreen
          key={record.view.terminal_id}
          record={record}
          active={visible && record.view.terminal_id === shown}
        />
      ))}
    </section>
  );
}
