import { useCallback, useEffect, useRef, useState } from 'react';
import type { AgentToolSettingsView, WebServiceSettingV1 } from '@aura/contracts/generated';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';

/** One message for every path where the save may have been applied: the user verifies first. */
const UNCONFIRMED_SAVE = '保存结果未确认。请重新打开工具设置核对后再操作。';

/**
 * The one owner of the committed tool-configuration snapshot.
 *
 * Both editors on the tool page — the default tool list and the execution
 * strategy — submit the whole configuration, because the update contract is one
 * revision-checked edit. If each read its own snapshot, saving in one would leave
 * the other holding a stale revision, and the second save would be rejected even
 * though nothing about it was wrong. One snapshot and one revision is what makes
 * any save order work.
 *
 * `commit` therefore rebuilds the whole request from this snapshot and applies
 * only the caller's own fields on top, so a save can neither carry a stale value
 * for a field the caller does not own nor invent a value for one it omitted.
 */
export interface ConfigurationUpdate {
  fixed_tool_ids?: string[];
  max_dynamic_tool_schemas?: number;
  max_concurrent_tool_calls?: number;
  default_timeout_ms?: number;
  max_retries?: number;
  /** Replaces the selection with the list this build recommends. */
  use_recommended_fixed_tool_ids?: true;
  /**
   * Present only when this save owns the proxy field. `null` clears it.
   * A strategy save omits the field and leaves the stored proxy alone.
   */
  network_proxy?: {
    enabled: boolean;
    protocol: 'http' | 'socks5';
    host: string;
    port: number;
  } | null;
  /** Present only when this save owns the web service choice. */
  web_service?: WebServiceSettingV1;
}

export interface ConfigurationOwner {
  view: AgentToolSettingsView | undefined;
  /** Set only when the read failed, so the UI can show a real error and retry. */
  loadError: string | undefined;
  busy: boolean;
  reload: () => Promise<void>;
  commit: (
    update: ConfigurationUpdate,
    expectedRevision?: string,
  ) => Promise<{ view?: AgentToolSettingsView; error?: string }>;
}

export function useAgentConfiguration(): ConfigurationOwner {
  const [view, setView] = useState<AgentToolSettingsView>();
  const [loadError, setLoadError] = useState<string>();
  const [busy, setBusy] = useState(false);
  // The authoritative revision for the next submit. A ref rather than the state
  // value, because two saves in quick succession would otherwise both read the
  // revision recorded before the first one completed.
  const revision = useRef<string | undefined>(undefined);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const accept = useCallback((next: AgentToolSettingsView) => {
    revision.current = next.revision;
    setView(next);
    setLoadError(undefined);
  }, []);

  /** The read is I/O only: it publishes nothing, so the effect that starts it can
   *  hand the result on without touching state itself. */
  const read = useCallback(async (): Promise<{ view?: AgentToolSettingsView; error?: string }> => {
    try {
      const result = await window.aura.v1.readAgentToolSettings({});
      return result.ok ? { view: result.value } : { error: result.error.message };
    } catch {
      return { error: '工具配置暂不可用。' };
    }
  }, []);

  /** The one place the read result reaches state, whichever call started it. */
  const publish = useCallback(
    (outcome: { view?: AgentToolSettingsView; error?: string }) => {
      if (!alive.current) return;
      if (outcome.view !== undefined) accept(outcome.view);
      else setLoadError(outcome.error ?? '工具配置暂不可用。');
    },
    [accept],
  );

  const reload = useCallback(async () => {
    publish(await read());
  }, [publish, read]);

  useEffect(() => {
    // Starting a read is pending work, not a state sync: the snapshot is published
    // from the promise continuation, so nothing here sets state while the effect
    // body runs.
    void read().then(publish);
  }, [publish, read]);

  const commit = useCallback(
    async (update: ConfigurationUpdate, expectedRevision?: string) => {
      const expected = revision.current;
      const base = view;
      if (expected === undefined || base === undefined) {
        return { error: '工具配置尚未读取完成，请重新打开工具设置。' };
      }
      if (expectedRevision !== undefined && expectedRevision !== expected) {
        return { error: '配置已更新，请重新读取并预览代理恢复。' };
      }
      setBusy(true);
      try {
        const restore = update.use_recommended_fixed_tool_ids === true;
        const request = {
          expected_revision: expected,
          // A restore asks Core for the published list, so the ids it sends are
          // ignored; every other field still comes from the shared snapshot.
          fixed_tool_ids: restore ? [] : (update.fixed_tool_ids ?? base.fixed_tool_ids),
          max_dynamic_tool_schemas:
            update.max_dynamic_tool_schemas ?? base.max_dynamic_tool_schemas,
          max_concurrent_tool_calls:
            update.max_concurrent_tool_calls ?? base.max_concurrent_tool_calls,
          default_timeout_ms: update.default_timeout_ms ?? base.default_timeout_ms,
          max_retries: update.max_retries ?? base.max_retries,
          use_recommended_fixed_tool_ids: restore,
        };
        const withProxy =
          'network_proxy' in update
            ? { ...request, network_proxy: update.network_proxy ?? null }
            : request;
        const result = await window.aura.v1.configureAgentTools(
          update.web_service === undefined
            ? withProxy
            : { ...withProxy, web_service: update.web_service },
        );
        if (!alive.current) return {};
        // A write that may already have been applied is not a definite failure: nothing here
        // re-sends it, and the message asks the user to confirm the stored value first.
        if (!result.ok) {
          return {
            error: isUnconfirmedCommandOutcome(result.error)
              ? UNCONFIRMED_SAVE
              : result.error.message,
          };
        }
        accept(result.value);
        return { view: result.value };
      } catch {
        if (!alive.current) return {};
        return { error: UNCONFIRMED_SAVE };
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [accept, view],
  );

  return { view, loadError, busy, reload, commit };
}
