import type { QueryAnswer, UserQuery } from '@aura/contracts/generated';
import type { CSSProperties } from 'react';
import { useState } from 'react';
import { AppIcon } from '../../app-icons';
import { userMessage } from '../user-message';
import { QUERY_COUNTDOWN_SECONDS, useQueryCountdown } from './query-countdown';
import { draftAnswer, type QuestionDraft } from './query-draft';
import { QueryQuestionBlock } from './QueryQuestion';
import './query.css';

/**
 * The model's question to the user. Every question takes the user's own
 * words; the user may decline; left alone for 30 s a countdown shows, and when it runs out the
 * question ends as "no response" so the rest of the task is not held up. The model is told which,
 * and never decides for the user.
 */
export function QueryForm({
  query,
  sessionId,
  deadlineAt,
  onChanged,
}: {
  query: UserQuery;
  sessionId: string;
  /** When Core ends the question as "no response" unless it is touched. */
  deadlineAt?: string | null | undefined;
  onChanged: () => void;
}) {
  const countdown = useQueryCountdown(query, sessionId, deadlineAt);
  const questions = query.input.questions;
  const [drafts, setDrafts] = useState<Record<string, QuestionDraft>>(() =>
    Object.fromEntries(
      questions.map((question) => [
        question.id,
        {
          custom: false,
          customText: '',
          value:
            question.answer_type.type === 'multi_choice'
              ? { answer: 'choices' as const, ids: [] }
              : undefined,
        },
      ]),
    ),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const send = async (answers: QueryAnswer[]) => {
    setBusy(true);
    try {
      const result = await window.aura.v1.answerAgentQuery({
        session_id: sessionId,
        agent_run_id: query.agent_run_id,
        query_id: query.query_id,
        expected_revision: query.revision,
        answers,
      });
      if (!result.ok) {
        setError(userMessage(result.error.message, '回答没有送达，请核对当前问题状态。'));
        return;
      }
      setError(undefined);
      onChanged();
    } catch {
      setError('回答结果未确认，请核对当前问题状态。');
      onChanged();
    } finally {
      setBusy(false);
    }
  };
  const submit = () => {
    const answers: QueryAnswer[] = [];
    for (const [index, question] of questions.entries()) {
      const draft = drafts[question.id] ?? { custom: false, customText: '' };
      const value = draftAnswer(question, draft);
      if (typeof value === 'string') {
        setError(questions.length > 1 ? `第 ${index + 1} 个问题：${value}` : value);
        return;
      }
      answers.push({ question_id: question.id, value });
    }
    void send(answers);
  };
  const decline = () =>
    void send(
      questions.map((question) => ({ question_id: question.id, value: { answer: 'declined' } })),
    );
  return (
    <form
      aria-label="回答当前问题"
      className="agent-query"
      onFocusCapture={countdown.touch}
      onKeyDownCapture={countdown.touch}
      onPointerDownCapture={countdown.touch}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <header className="agent-query-header">
        <span className="agent-query-mark" aria-hidden="true">
          <AppIcon name="chat" size={14} />
        </span>
        <strong>需要你的回答</strong>
        <span className="agent-query-kind">
          {questions.length > 1 ? `${questions.length} 个问题` : 'Aura 在等你回复'}
        </span>
        {countdown.visible ? <QueryCountdownChip remaining={countdown.remaining ?? 0} /> : null}
      </header>
      <fieldset className="agent-query-body" disabled={busy}>
        {questions.map((question, index) => (
          <QueryQuestionBlock
            draft={drafts[question.id] ?? { custom: false, customText: '' }}
            index={index}
            key={question.id}
            onDraft={(draft) => {
              setError(undefined);
              setDrafts((old) => ({ ...old, [question.id]: draft }));
            }}
            query={query}
            question={question}
            sessionId={sessionId}
          />
        ))}
      </fieldset>
      {error ? (
        <p className="agent-query-error" role="alert">
          {error}
        </p>
      ) : null}
      <footer className="agent-query-actions">
        <button className="button is-ghost" disabled={busy} onClick={decline} type="button">
          拒绝回答
        </button>
        <button className="button is-primary" disabled={busy} type="submit">
          {busy ? '提交中…' : '提交回答'}
        </button>
      </footer>
    </form>
  );
}

/** "28 秒后视为暂时没有回应", with a ring that empties as the time goes. */
function QueryCountdownChip({ remaining }: { remaining: number }) {
  const left = Math.min(1, Math.max(0, remaining / QUERY_COUNTDOWN_SECONDS));
  return (
    <span
      className="agent-query-countdown"
      data-ending={remaining <= 10 || undefined}
      role="timer"
      title="一直没有操作这个提问时，到时 Aura 会告诉 AI 你可能在忙、先做不受影响的工作；任何点击、选择或输入都会重新计时。"
    >
      <span
        aria-hidden="true"
        className="agent-query-countdown-ring"
        style={{ '--left': left } as CSSProperties}
      />
      {remaining > 0 ? `${remaining} 秒后视为暂时没有回应` : '正在告诉 AI 你暂时没有回应…'}
    </span>
  );
}
