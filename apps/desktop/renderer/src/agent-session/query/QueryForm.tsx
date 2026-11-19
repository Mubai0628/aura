import type { QueryAnswer, UserQuery } from '@aura/contracts/generated';
import { useMemo, useState } from 'react';
import { AppIcon } from '../../app-icons';
import { connectionCause, failedAction } from '../failure-text';
import { useHumanWait } from '../human-wait';
import { WaitCountdown } from '../WaitCountdown';
import { draftAnswer, type QuestionDraft } from './query-draft';
import { QueryPager } from './QueryPager';
import { QueryQuestionBlock } from './QueryQuestion';
import './query.css';

/** Core's refusals that mean the question is no longer waiting for this answer. */
const ENDED = ['agent.query_missing', 'agent.query_changed'];

const EMPTY: QuestionDraft = { custom: false, customText: '' };

/**
 * The model's question to the user. Every question takes the user's own
 * words; the user may decline; left alone for 30 s a countdown shows, and when it runs out the
 * question ends as "no response" so the rest of the task is not held up. The model is told which,
 * and never decides for the user. Several questions show one at a time, the user moving between
 * them; the answers go once every one is answered.
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
  const target = useMemo(
    () => ({
      kind: 'query' as const,
      agent_run_id: query.agent_run_id,
      query_id: query.query_id,
    }),
    [query.agent_run_id, query.query_id],
  );
  const {
    mount,
    focus,
    blur,
    touch,
    deadlineAt: waitDeadline,
    held,
    focusedUntil,
  } = useHumanWait<HTMLFormElement>(sessionId, target);
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
  const [shown, setShown] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const answered = questions.map(
    (question) => typeof draftAnswer(question, drafts[question.id] ?? EMPTY) !== 'string',
  );
  const current = questions[Math.min(shown, questions.length - 1)];
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
        setError(
          ENDED.some((code) => `${result.error.code ?? ''} ${result.error.message}`.includes(code))
            ? '这个问题已经结束（超时、任务已停止或问题已更新），你的回答没有送达；请看最新的问题，或在输入框说明。'
            : failedAction(result.error, '回答没有送达', '问题仍在等你回答；请再提交一次。'),
        );
        return;
      }
      setError(undefined);
      onChanged();
    } catch (reason) {
      const cause = connectionCause(reason) ?? '与 Aura Core 的连接出了问题';
      setError(`回答是否送达没有确认（${cause}），没有自动重发；请看问题是否还在等你回答。`);
      onChanged();
    } finally {
      setBusy(false);
    }
  };
  const submit = () => {
    const answers: QueryAnswer[] = [];
    for (const [index, question] of questions.entries()) {
      const value = draftAnswer(question, drafts[question.id] ?? EMPTY);
      if (typeof value === 'string') {
        setShown(index);
        setError(questions.length > 1 ? `第 ${index + 1} 题：${value}` : value);
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
      ref={mount}
      onFocusCapture={focus}
      onBlurCapture={blur}
      onKeyDownCapture={touch}
      onPointerDownCapture={touch}
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
          {questions.length > 1
            ? `第 ${Math.min(shown, questions.length - 1) + 1} / ${questions.length} 题`
            : 'Aura 在等你回复'}
        </span>
        <WaitCountdown
          deadlineAt={waitDeadline ?? deadlineAt}
          held={held}
          focusedUntil={focusedUntil}
        />
      </header>
      {/* A div scrolls; the question's own fieldset disables its controls while sending. */}
      <div className="agent-query-body">
        {current ? (
          <QueryQuestionBlock
            disabled={busy}
            draft={drafts[current.id] ?? EMPTY}
            index={questions.indexOf(current)}
            key={current.id}
            onDraft={(draft) => {
              setError(undefined);
              setDrafts((old) => ({ ...old, [current.id]: draft }));
            }}
            query={query}
            question={current}
            sessionId={sessionId}
          />
        ) : null}
      </div>
      {error ? (
        <p className="agent-query-error" role="alert">
          {error}
        </p>
      ) : null}
      <footer className="agent-query-actions">
        {questions.length > 1 ? (
          <QueryPager
            answered={answered}
            count={questions.length}
            current={Math.min(shown, questions.length - 1)}
            disabled={busy}
            onShow={(index) => {
              setError(undefined);
              setShown(index);
            }}
          />
        ) : null}
        <button className="button is-ghost" disabled={busy} onClick={decline} type="button">
          拒绝回答
        </button>
        <button
          className="button is-primary"
          disabled={busy || answered.includes(false)}
          type="submit"
        >
          {busy ? '提交中…' : '提交回答'}
        </button>
      </footer>
    </form>
  );
}
