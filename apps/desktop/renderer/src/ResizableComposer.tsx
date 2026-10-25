import type {
  ContextUsageSummaryView,
  PermissionMode,
  PermissionModeTransition,
  PromptInspectorProjection,
  SessionModelSelectorProjection,
} from '@aura/contracts/generated';
import type { CommandEntry } from '@aura/contracts/generated';
import type { CSSProperties, KeyboardEvent, PointerEvent } from 'react';
import { useContext, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import { ComposerModeChip } from './ComposerModeChip';
import { useCompactRow } from './ui/useCompactRow';
import { ModelQuickSelector, PermissionQuickSelector } from './ComposerQuickMenus';
import { ContextUsageRing } from './ContextUsageRing';
import { ModelPicker, type ModelPickerState } from './ModelPicker';
import { SessionReasoningPicker } from './ReasoningPicker';
import type { InspectionSource } from './agent-session/useLatestInspection';
import type { SubmissionPreparation } from './agent-session/useComposerSubmission';
import { useDismissablePopover } from './useDismissablePopover';
import { useComposerSubmission } from './agent-session/useComposerSubmission';
import { CommandMenu } from './composer-commands/CommandMenu';
import { SkillChips, SkillPicker } from './composer-skills/SkillChoice';
import { useSkillChoice } from './composer-skills/useSkillChoice';
import { useCommandCatalog } from './composer-commands/useCommandCatalog';
import { useSlashCommands } from './composer-commands/useSlashCommands';
import { userMessage } from './agent-session/user-message';
import { ComposerAttachments } from './composer-attachments/ComposerAttachments';
import type { FileActionScope } from './files/file-menu';
import { attachmentDrafts } from './composer-attachments/attachment-files';
import { ConversationAttachmentState } from './composer-attachments/ConversationAttachments';
import { ConversationAttachmentContext } from './composer-attachments/conversation-attachment-context';

const MIN_EDITOR_HEIGHT = 54;
const MAX_EDITOR_HEIGHT = 260;

type ComposerStyle = CSSProperties & { '--composer-editor-height': string };

interface ResizableComposerProps {
  sessionId: string;
  draftModelPicker?: ModelPickerState;
  preparation?: SubmissionPreparation;
  activeTurn?: string | undefined;
  queuePending: boolean;
  onChanged: () => void;
  onStop: () => Promise<void>;
  demoMode: boolean;
  modelSelector: SessionModelSelectorProjection | undefined;
  modelSelectorError?: string | undefined;
  onOpenModelSettings: () => void;
  onOpenPromptInspector: () => void;
  onOpenSecuritySettings: () => void;
  onSelectModel: (modelProfileId: string) => Promise<void>;
  onSetPermissionMode: (mode: PermissionMode) => Promise<void>;
  permissionMode: PermissionMode | undefined;
  permissionTransition?: PermissionModeTransition | undefined;
  contextUsageSource?: InspectionSource | undefined;
  promptInspector: PromptInspectorProjection | undefined;
  contextUsage?: ContextUsageSummaryView | undefined;
  compactionRunning?: boolean;
  /** Commands: the workspace for its prompt commands, the run `/compact` applies to. */
  workspaceId?: string | undefined;
  activeRunId?: string | undefined;
  onOpenSettings?: ((section: 'tools' | 'memory') => void) | undefined;
  onOpenPanel?: ((panel: 'files' | 'review' | 'terminal' | 'process') => void) | undefined;
  /** The conversation's workspace files, for the menu of a file added to the message. */
  fileScope?: FileActionScope | undefined;
}

const PANELS = {
  files: 'files',
  diff: 'review',
  terminal: 'terminal',
  process: 'process',
} as const;

export function ResizableComposer(props: ResizableComposerProps) {
  const shared = useContext(ConversationAttachmentContext);
  return shared ? (
    <Composer {...props} />
  ) : (
    <ConversationAttachmentState>
      <Composer {...props} />
    </ConversationAttachmentState>
  );
}

function Composer({
  sessionId,
  draftModelPicker,
  preparation,
  activeTurn,
  queuePending,
  onChanged,
  onStop,
  demoMode,
  modelSelector,
  modelSelectorError,
  onOpenModelSettings,
  onOpenPromptInspector,
  onOpenSecuritySettings,
  onSelectModel,
  onSetPermissionMode,
  permissionMode,
  permissionTransition,
  contextUsageSource,
  promptInspector,
  contextUsage: contextUsageProp,
  compactionRunning = false,
  workspaceId,
  activeRunId,
  onOpenSettings,
  onOpenPanel,
  fileScope,
}: ResizableComposerProps) {
  // Too narrow for the permission and model words, those buttons show their icons.
  const controls = useCompactRow<HTMLDivElement>();
  const attachmentMenuRef = useRef<HTMLDivElement>(null);
  const [attachmentMenuOpen, setAttachmentMenuOpen] = useDismissablePopover(attachmentMenuRef);
  const submission = useComposerSubmission(
    sessionId,
    activeTurn,
    draftModelPicker?.selectedId ?? modelSelector?.selected_model_profile_id ?? undefined,
    queuePending,
    onChanged,
    preparation,
  );
  const { draft, setDraft } = submission;
  // The thinking level of the model a new conversation is about to use.
  const draftChoice = draftModelPicker?.choices.find(
    (choice) => choice.id === draftModelPicker.selectedId,
  );
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [modelOpenSignal, setModelOpenSignal] = useState(0);
  const [permissionOpenSignal, setPermissionOpenSignal] = useState(0);
  const [commandNotice, setCommandNotice] = useState<{ alert: boolean; text: string }>();
  const catalog = useCommandCatalog(workspaceId);
  const skills = useSkillChoice(workspaceId);
  // Pictures and files from the + menu, a paste or a drop.
  const attached = useContext(ConversationAttachmentContext)!;
  const filePicker = useRef<HTMLInputElement>(null);
  const runImmediate = (command: CommandEntry) => {
    setCommandNotice(undefined);
    switch (command.name) {
      case 'model':
        setModelOpenSignal((n) => n + 1);
        return;
      case 'permission':
        setPermissionOpenSignal((n) => n + 1);
        return;
      case 'stop':
        void onStop();
        return;
      case 'context':
        onOpenPromptInspector();
        return;
      case 'tools':
      case 'memory':
        onOpenSettings?.(command.name);
        return;
      case 'files':
      case 'diff':
      case 'terminal':
      case 'process':
        onOpenPanel?.(PANELS[command.name]);
        return;
      case 'help':
        slash.browse();
        return;
      // The task modes come from their commands, not the + menu.
      case 'plan':
      case 'goal':
        submission.setMode(command.name === 'plan' ? 'plan' : 'goal');
        return;
    }
  };
  const slash = useSlashCommands({
    textarea: textareaRef,
    draft,
    setDraft,
    commands: catalog.commands,
    context: {
      session: draftModelPicker === undefined,
      activeTurn: Boolean(activeTurn),
      queuePending,
      modelSelected: Boolean(
        draftModelPicker?.selectedId ?? modelSelector?.selected_model_profile_id,
      ),
    },
    onImmediate: runImmediate,
    onOpen: catalog.reload,
  });
  /** A command block runs its own action; it is never queued or appended as a message. */
  const runBlock = async (block: CommandEntry) => {
    setCommandNotice(undefined);
    if (block.name === 'init') {
      if (await submission.sendInit()) slash.setBlock(undefined);
      return;
    }
    if (block.name === 'compact') {
      // With nothing running, /compact is a task of its own that compacts first.
      if (!activeRunId) {
        if (await submission.sendCompaction()) slash.setBlock(undefined);
        return;
      }
      const result = await window.aura.v1.requestAgentCompaction({
        session_id: sessionId,
        agent_run_id: activeRunId,
        focus: draft.trim() || null,
      });
      if (!result.ok) {
        setCommandNotice({
          alert: true,
          text: userMessage(result.error.message, '压缩没有登记，请稍后再试。'),
        });
        return;
      }
      setDraft('');
      slash.setBlock(undefined);
      setCommandNotice({ alert: false, text: '已登记压缩，任务到安全点时执行。' });
    }
  };
  // While a task runs, a message waits in the queue; the queue appends it at once.
  const send = () => {
    if (slash.block) void runBlock(slash.block);
    else
      void submission
        .send(
          skills.chosen.map((skill) => skill.id),
          attachmentDrafts(attached.attachments),
        )
        .then((sent) => {
          if (!sent) return;
          skills.clear();
          attached.clear();
        });
  };
  const empty = !draft.trim() && !slash.block && attached.attachments.length === 0;
  const [editorHeight, setEditorHeight] = useState(68);
  const drag = useRef<{ pointerId: number; startHeight: number; startY: number } | undefined>(
    undefined,
  );

  const clampHeight = (height: number) =>
    Math.min(MAX_EDITOR_HEIGHT, Math.max(MIN_EDITOR_HEIGHT, Math.round(height)));

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    drag.current = {
      pointerId: event.pointerId,
      startHeight: editorHeight,
      startY: event.clientY,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    if (active === undefined || active.pointerId !== event.pointerId) return;
    setEditorHeight(clampHeight(active.startHeight + active.startY - event.clientY));
  };

  const finishResize = (element: HTMLElement) => {
    const active = drag.current;
    if (active !== undefined && element.hasPointerCapture(active.pointerId)) {
      element.releasePointerCapture(active.pointerId);
    }
    drag.current = undefined;
  };

  const handleResizeKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 32 : 12;
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setEditorHeight((current) => clampHeight(current + step));
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setEditorHeight((current) => clampHeight(current - step));
    }
  };

  const style: ComposerStyle = { '--composer-editor-height': `${editorHeight}px` };
  const contextUsage = promptInspector?.context_usage ?? contextUsageProp;

  return (
    <section aria-label="消息编辑器" className="composer" style={style}>
      <div
        aria-label="调整消息编辑器高度"
        aria-orientation="horizontal"
        aria-valuemax={MAX_EDITOR_HEIGHT}
        aria-valuemin={MIN_EDITOR_HEIGHT}
        aria-valuenow={editorHeight}
        className="composer-resize-handle"
        onKeyDown={handleResizeKeyDown}
        onPointerCancel={(event) => finishResize(event.currentTarget)}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={(event) => finishResize(event.currentTarget)}
        role="separator"
        tabIndex={0}
      />
      <label className="sr-only" htmlFor="aura-composer">
        输入消息
      </label>
      {slash.block ? (
        <div className="composer-command-block" role="group" aria-label="命令块">
          <span className="command-name">/{slash.block.name}</span>
          <span>{slash.block.title}</span>
          <button
            type="button"
            aria-label={`移除命令 /${slash.block.name}`}
            onClick={() => slash.setBlock(undefined)}
          >
            <AppIcon name="close" size={13} />
          </button>
        </div>
      ) : null}
      <SkillChips chosen={skills.chosen} onRemove={skills.remove} />
      <ComposerAttachments
        attachments={attached.attachments}
        error={attached.error}
        onRemove={attached.remove}
        scope={fileScope}
      />
      <textarea
        id="aura-composer"
        ref={textareaRef}
        onChange={(event) => {
          setDraft(event.target.value);
          slash.sync(event.target.value, event.target.selectionStart);
        }}
        onSelect={(event) =>
          slash.sync(event.currentTarget.value, event.currentTarget.selectionStart)
        }
        disabled={submission.busy}
        onPaste={(event) => {
          if (!event.clipboardData.files.length) return;
          event.preventDefault();
          void attached.add([...event.clipboardData.files]);
        }}
        onKeyDown={(event) => {
          if (slash.keyDown(event)) return;
          if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            send();
          }
        }}
        placeholder={slash.block?.argument_hint ?? '向 Aura 提问，输入 / 使用命令…'}
        value={draft}
      />
      {skills.picker ? (
        <SkillPicker
          picker={skills.picker}
          chosen={skills.chosen}
          onChoose={skills.add}
          onClose={skills.close}
        />
      ) : null}
      {slash.menu ? (
        <CommandMenu
          options={slash.options}
          highlight={slash.menu.highlight}
          error={catalog.error}
          onPick={slash.pick}
          onHover={slash.hover}
          onDismiss={slash.close}
        />
      ) : null}
      {commandNotice ? (
        <p role={commandNotice.alert ? 'alert' : 'status'}>{commandNotice.text}</p>
      ) : null}
      {submission.error ? <p role="alert">{submission.error}</p> : null}
      {submission.needsReview ? (
        <div role="status">
          发送结果待核对，未自动重试。
          <button type="button" onClick={onChanged}>
            刷新历史与队列
          </button>
          <button type="button" onClick={submission.acknowledge}>
            已核对，返回编辑
          </button>
        </div>
      ) : null}
      <div className="composer-controls" ref={controls}>
        <div className="composer-controls-leading">
          <div className="attachment-menu-anchor" ref={attachmentMenuRef}>
            <button
              aria-expanded={attachmentMenuOpen}
              aria-label="添加"
              aria-haspopup="menu"
              className="icon-button composer-square-button"
              onClick={() => setAttachmentMenuOpen((open) => !open)}
              type="button"
            >
              <AppIcon name="plus" size={16} />
            </button>
            {attachmentMenuOpen ? (
              <div
                className="attachment-menu composer-popover mode-popover"
                role="menu"
                aria-label="添加"
              >
                <p>添加</p>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setAttachmentMenuOpen(false);
                    filePicker.current?.click();
                  }}
                >
                  <span>图片或文件</span>
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setAttachmentMenuOpen(false);
                    slash.browse();
                    textareaRef.current?.focus();
                  }}
                >
                  <span>命令</span>
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setAttachmentMenuOpen(false);
                    void skills.open();
                  }}
                >
                  <span>Skill</span>
                </button>
              </div>
            ) : null}
            <input
              aria-label="选择图片或文件"
              className="sr-only"
              multiple
              onChange={(event) => {
                const files = [...(event.target.files ?? [])];
                event.target.value = '';
                void attached.add(files);
              }}
              ref={filePicker}
              tabIndex={-1}
              type="file"
            />
          </div>

          {!activeTurn && !queuePending && submission.mode !== 'normal' ? (
            <ComposerModeChip
              budgetTokens={submission.budgetTokens}
              budgetTurns={submission.budgetTurns}
              mode={submission.mode}
              onBudgetTokens={submission.setBudgetTokens}
              onBudgetTurns={submission.setBudgetTurns}
              onLeave={() => submission.setMode('normal')}
            />
          ) : null}
          <PermissionQuickSelector
            openSignal={permissionOpenSignal}
            demoMode={demoMode}
            mode={permissionMode}
            transition={permissionTransition}
            onManageSettings={onOpenSecuritySettings}
            onSelect={onSetPermissionMode}
          />
        </div>

        <div className="composer-controls-trailing">
          <div className="composer-context">
            <ContextUsageRing
              onOpenInspector={onOpenPromptInspector}
              running={promptInspector?.compaction.state === 'running' || compactionRunning}
              source={contextUsageSource}
              usage={contextUsage}
            />
          </div>

          {draftModelPicker ? (
            <>
              <ModelPicker
                openSignal={modelOpenSignal}
                state={draftModelPicker}
                onSelect={onSelectModel}
                onManageSettings={onOpenModelSettings}
              />
              {draftChoice ? (
                <SessionReasoningPicker
                  levels={draftChoice.reasoning?.levels ?? []}
                  modelProfileId={draftChoice.id}
                  sessionId={sessionId}
                />
              ) : null}
            </>
          ) : (
            <ModelQuickSelector
              openSignal={modelOpenSignal}
              onManageSettings={onOpenModelSettings}
              onSelect={onSelectModel}
              projection={modelSelector}
              error={modelSelectorError}
            />
          )}

          {activeTurn && empty ? (
            <button
              aria-label="停止当前任务"
              className="send-button is-stop"
              title="停止当前任务"
              onClick={() => void onStop()}
              type="button"
            >
              <span aria-hidden="true" className="send-stop-square" />
            </button>
          ) : (
            <button
              aria-label={activeTurn || queuePending ? '加入下一轮队列' : '发送消息'}
              className="send-button"
              disabled={empty || submission.busy || submission.needsReview}
              title={activeTurn || queuePending ? '加入下一轮队列' : '发送消息'}
              onClick={() => send()}
              type="button"
            >
              <AppIcon name="send" size={14} />
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
