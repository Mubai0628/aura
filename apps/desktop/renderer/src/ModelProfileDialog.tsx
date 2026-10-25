import type {
  ModelEndpointConfig,
  ModelProfileConfig,
  ModelSettingsProjection,
  ReasoningEffort,
} from '@aura/contracts/generated';
import { useState } from 'react';
import * as Collapsible from '@radix-ui/react-collapsible';

import { AppIcon } from './app-icons';
import { ModalDialog } from './ModalDialog';
import { ToggleSwitch } from './ui/ToggleSwitch';
import './model-dialogs.css';
import type { NotificationInput } from './notification-center-store';
import { userMessage } from './agent-session/user-message';
import { MODEL_FAILURE_FALLBACK } from './provider-connection/notices';
import { REASONING_LABELS, levelToSend } from './reasoning-effort';
import './styles/model-picker.css';

interface ModelProfileDialogProps {
  endpoint: ModelEndpointConfig;
  onClose: () => void;
  onNotify: (notification: NotificationInput) => void;
  onSaved: () => Promise<void>;
  profile?: ModelProfileConfig;
  settings: ModelSettingsProjection;
  writeUnavailableReason?: string | undefined;
}

/** Every thinking level, lowest first, in the order the settings list them. */
const ALL_LEVELS: readonly ReasoningEffort[] = [
  'off',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
];

export function ModelProfileDialog({
  endpoint,
  onClose,
  onNotify,
  onSaved,
  profile,
  settings,
  writeUnavailableReason,
}: ModelProfileDialogProps) {
  const [providerModelName, setProviderModelName] = useState(profile?.provider_model_name ?? '');
  const [displayName, setDisplayName] = useState(profile?.display_name ?? '');
  const [notes, setNotes] = useState(profile?.notes ?? '');
  const [contextWindow, setContextWindow] = useState<number | ''>(
    profile?.context.user_override_window_tokens ?? profile?.context.catalog_window_tokens ?? '',
  );
  const [maxOutputTokens, setMaxOutputTokens] = useState<number | ''>(
    profile?.request_defaults.max_output_tokens ?? '',
  );
  const [historicalTokens, setHistoricalTokens] = useState(
    profile?.context.historical_snapshot_max_tokens ?? 1024,
  );
  const [temperature, setTemperature] = useState<number | ''>(
    profile?.request_defaults.temperature ?? '',
  );
  const [topP, setTopP] = useState<number | ''>(profile?.request_defaults.top_p ?? '');
  // The thinking levels this model offers, and the one its profile names.
  const reasoningLevels =
    settings.reasoning_efforts?.find((entry) => entry.model_profile_id === profile?.id)
      ?.reasoning_efforts ?? [];
  const storedLevel = profile?.request_defaults.reasoning_effort ?? null;
  // Shown as the level the model is sent.
  const [reasoningEffort, setReasoningEffort] = useState<ReasoningEffort | null>(
    storedLevel === null ? null : levelToSend(storedLevel, reasoningLevels),
  );
  // The levels the model offers can be listed here, for a
  // model the catalog does not know or knows wrongly; once changed, the list is the profile's own.
  const listedLevels = profile?.request_defaults.reasoning_efforts ?? null;
  const [offered, setOffered] = useState<ReasoningEffort[]>(listedLevels ?? reasoningLevels);
  const [ownList, setOwnList] = useState(listedLevels !== null);
  const toggleLevel = (level: ReasoningEffort) => {
    const next = offered.includes(level)
      ? offered.filter((each) => each !== level)
      : ALL_LEVELS.filter((each) => each === level || offered.includes(each));
    setOwnList(true);
    setOffered(next);
    if (reasoningEffort !== null && !next.includes(reasoningEffort)) setReasoningEffort(null);
  };
  // Whether the model is sent images (the browser's screenshots). Unset sends them, unless the catalog knows the model takes none.
  const [imageInput, setImageInput] = useState<boolean | null>(
    profile?.request_defaults.image_input ?? null,
  );
  const catalogImages = !(
    profile?.catalog?.match_state === 'matched' &&
    !profile.catalog.snapshot.input_modalities.includes('image')
  );
  const [enabled, setEnabled] = useState(profile?.enabled ?? true);
  const [busy, setBusy] = useState(false);
  const titleId = `model-profile-${profile?.id ?? endpoint.id}`;
  const valid =
    providerModelName.length > 0 &&
    displayName.trim().length > 0 &&
    (contextWindow === '' || contextWindow > 0) &&
    (maxOutputTokens === '' || maxOutputTokens > 0) &&
    historicalTokens >= 0;

  const save = async () => {
    if (!valid || busy || writeUnavailableReason !== undefined) return;
    setBusy(true);
    const result =
      profile === undefined
        ? await window.aura.v1.createModelProfile({
            expected_revision: settings.models_revision,
            input: {
              context_window_tokens: contextWindow === '' ? null : contextWindow,
              display_name: displayName,
              endpoint_id: endpoint.id,
              notes: notes === '' ? null : notes,
              provider_model_name: providerModelName,
            },
          })
        : await window.aura.v1.updateModelProfile({
            display_name: displayName,
            enabled,
            expected_revision: settings.models_revision,
            historical_snapshot_max_tokens: historicalTokens,
            model_profile_id: profile.id,
            notes: notes === '' ? null : notes,
            request_defaults: {
              chat_instruction_role: profile.request_defaults.chat_instruction_role ?? null,
              chat_output_token_field: profile.request_defaults.chat_output_token_field ?? null,
              max_output_tokens: maxOutputTokens === '' ? null : maxOutputTokens,
              temperature: temperature === '' ? null : temperature,
              top_p: topP === '' ? null : topP,
              reasoning_effort: reasoningEffort,
              ...(ownList ? { reasoning_efforts: offered } : {}),
              image_input: imageInput,
            },
          });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, 'Model Profile 未保存', result.error);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `model-profile-saved:${result.value.value.id}:${result.value.revision}`,
      message: `${result.value.value.display_name} 已写入 Core 模型配置。`,
      title: 'Model Profile 已保存',
      tone: 'success',
    });
    onClose();
  };

  const correctContext = async () => {
    if (
      profile === undefined ||
      contextWindow === '' ||
      contextWindow <= 0 ||
      busy ||
      writeUnavailableReason !== undefined
    ) {
      return;
    }
    setBusy(true);
    const result = await window.aura.v1.correctModelContextWindow({
      expected_revision: settings.models_revision,
      model_profile_id: profile.id,
      window_tokens: contextWindow,
    });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, '上下文窗口未更新', result.error);
      return;
    }
    await onSaved();
  };

  const clearContext = async (runtime: boolean) => {
    if (profile === undefined || busy || writeUnavailableReason !== undefined) return;
    setBusy(true);
    const result = runtime
      ? await window.aura.v1.resetModelRuntimeContextWindow({
          expected_revision: settings.models_revision,
          model_profile_id: profile.id,
        })
      : await window.aura.v1.clearModelContextWindowOverride({
          expected_revision: settings.models_revision,
          model_profile_id: profile.id,
        });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, '上下文窗口未重置', result.error);
      return;
    }
    await onSaved();
    setContextWindow('');
  };

  return (
    <ModalDialog
      className="provider-dialog model-profile-dialog"
      onClose={busy ? () => undefined : onClose}
      titleId={titleId}
    >
      <header className="modal-header">
        <div className="profile-dialog-heading">
          <span className="dialog-feature-icon">
            <AppIcon name="model" size={22} />
          </span>
          <div>
            <h2 id={titleId}>{profile === undefined ? '手工添加模型' : '编辑模型'}</h2>
            <p>{endpoint.display_name}</p>
          </div>
        </div>
        <button
          aria-label="关闭模型编辑"
          className="icon-button"
          disabled={busy}
          onClick={onClose}
          type="button"
        >
          <AppIcon name="close" />
        </button>
      </header>
      <form
        className="provider-editor model-profile-editor"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        {writeUnavailableReason === undefined ? null : (
          <div className="model-settings-reconnect-note" role="status">
            <AppIcon name="activity" size={15} />
            <span>{writeUnavailableReason}</span>
          </div>
        )}
        <div className="profile-fields">
          <label>
            <span>Provider 模型标识</span>
            <input
              disabled={profile !== undefined}
              onChange={(event) => setProviderModelName(event.target.value)}
              type="text"
              value={providerModelName}
            />
          </label>
          <label>
            <span>显示名称</span>
            <input onChange={(event) => setDisplayName(event.target.value)} value={displayName} />
          </label>
          <label>
            <span>上下文窗口 Tokens</span>
            <input
              min={1}
              onChange={(event) => setContextWindow(numberOrBlank(event.target.value))}
              type="number"
              value={contextWindow}
            />
          </label>
          {profile === undefined ? (
            <p className="span-two settings-form-hint">
              创建后可继续配置最大输出、采样参数和历史快照上限；这些字段不会在创建阶段被静默忽略。
            </p>
          ) : (
            <>
              <label>
                <span>最大输出 Tokens</span>
                <input
                  min={1}
                  onChange={(event) => setMaxOutputTokens(numberOrBlank(event.target.value))}
                  type="number"
                  value={maxOutputTokens}
                />
              </label>
              <label>
                <span>历史快照 Tokens</span>
                <input
                  min={0}
                  onChange={(event) => setHistoricalTokens(Number(event.target.value))}
                  type="number"
                  value={historicalTokens}
                />
              </label>
              <label>
                <span>Temperature（可选）</span>
                <input
                  onChange={(event) => setTemperature(numberOrBlank(event.target.value))}
                  step="any"
                  type="number"
                  value={temperature}
                />
              </label>
              <label>
                <span>Top P（可选）</span>
                <input
                  onChange={(event) => setTopP(numberOrBlank(event.target.value))}
                  step="any"
                  type="number"
                  value={topP}
                />
              </label>
              {/* Pills that tick on and
                  off, and the default level as one choice on a segmented track. */}
              <fieldset className="span-two reasoning-levels">
                <legend>可选思考档位</legend>
                <div className="level-pills">
                  {ALL_LEVELS.map((level) => {
                    const on = offered.includes(level);
                    return (
                      <label className="level-pill" data-on={on} key={level}>
                        <input checked={on} onChange={() => toggleLevel(level)} type="checkbox" />
                        {on ? <AppIcon name="check" size={12} /> : null}
                        <span>{REASONING_LABELS[level]}</span>
                      </label>
                    );
                  })}
                </div>
              </fieldset>
              {offered.length ? (
                <div className="span-two level-choice">
                  <span id={`${titleId}-level`}>会话默认思考程度</span>
                  <div
                    aria-labelledby={`${titleId}-level`}
                    className="level-segments"
                    role="radiogroup"
                  >
                    {[null, ...offered].map((level) => (
                      <label className="level-segment" key={level ?? 'default'}>
                        <input
                          checked={reasoningEffort === level}
                          name={`${titleId}-level`}
                          onChange={() => setReasoningEffort(level)}
                          type="radio"
                        />
                        <span>{level === null ? '官方默认' : REASONING_LABELS[level]}</span>
                      </label>
                    ))}
                  </div>
                </div>
              ) : null}
              <div className="span-two level-choice">
                <span id={`${titleId}-images`}>图片输入</span>
                <div
                  aria-labelledby={`${titleId}-images`}
                  className="level-segments"
                  role="radiogroup"
                >
                  {(
                    [
                      [null, `自动（${catalogImages ? '接收' : '不接收'}）`],
                      [true, '接收'],
                      [false, '不接收'],
                    ] as const
                  ).map(([value, label]) => (
                    <label className="level-segment" key={String(value)}>
                      <input
                        checked={imageInput === value}
                        name={`${titleId}-images`}
                        onChange={() => setImageInput(value)}
                        type="radio"
                      />
                      <span>{label}</span>
                    </label>
                  ))}
                </div>
              </div>
              <div className="span-two profile-enabled-row">
                <div>
                  <strong>用于会话模型选择</strong>
                  <small>在会话的模型列表中显示此模型</small>
                </div>
                <ToggleSwitch
                  label="允许用于会话模型选择"
                  checked={enabled}
                  disabled={busy || writeUnavailableReason !== undefined}
                  onCheckedChange={setEnabled}
                />
              </div>
            </>
          )}
          <label className="span-two">
            <span>备注</span>
            <textarea
              rows={3}
              placeholder="添加备注，方便辨认此模型（可选）"
              maxLength={4096}
              onChange={(event) => setNotes(event.target.value)}
              value={notes}
            />
          </label>
        </div>
        {profile === undefined ? null : (
          <Collapsible.Root className="profile-context-tools">
            <Collapsible.Trigger className="context-tools-trigger">
              <span>上下文窗口修正</span>
              <AppIcon name="chevron-down" size={14} />
            </Collapsible.Trigger>
            <Collapsible.Content>
              <p>单独保存上方的上下文窗口值，或清除已有修正。</p>
              <div className="context-tools-actions">
                <button
                  disabled={busy || contextWindow === '' || writeUnavailableReason !== undefined}
                  onClick={() => void correctContext()}
                  type="button"
                >
                  应用上下文修正
                </button>
                <button
                  disabled={busy || writeUnavailableReason !== undefined}
                  onClick={() => void clearContext(false)}
                  type="button"
                >
                  清除用户修正
                </button>
                <button
                  disabled={busy || writeUnavailableReason !== undefined}
                  onClick={() => void clearContext(true)}
                  type="button"
                >
                  重置 Runtime 修正
                </button>
              </div>
            </Collapsible.Content>
          </Collapsible.Root>
        )}
        <footer className="modal-actions">
          <button disabled={busy} onClick={onClose} type="button">
            取消
          </button>
          <span className="modal-action-spacer" />
          <span
            className="disabled-action-wrapper"
            tabIndex={writeUnavailableReason === undefined ? -1 : 0}
          >
            <button
              className="primary-button"
              disabled={!valid || busy || writeUnavailableReason !== undefined}
              type="submit"
            >
              {busy ? '保存中…' : '保存 Profile'}
            </button>
            {writeUnavailableReason === undefined ? null : (
              <span role="tooltip">{writeUnavailableReason}</span>
            )}
          </span>
        </footer>
      </form>
    </ModalDialog>
  );
}

const numberOrBlank = (value: string): number | '' => (value === '' ? '' : Number(value));

const notifyFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${title}:${error.code}`,
    message: userMessage(error, MODEL_FAILURE_FALLBACK),
    title,
    tone: 'danger',
  });
};
