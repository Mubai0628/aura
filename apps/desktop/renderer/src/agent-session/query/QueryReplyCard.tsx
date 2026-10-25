import './query.css';
import type { QueryReplyView } from './query-reply';

const HEADINGS: Record<QueryReplyView['outcome'], string> = {
  answered: '你的回答',
  declined: '你拒绝了回答',
  no_response: '提问无人回应（已超时）',
};

const NOTES: Partial<Record<QueryReplyView['outcome'], string>> = {
  declined: 'Aura 已告诉 AI：你不回答这些问题，不要再追问，也不要替你决定。',
  no_response:
    'Aura 已告诉 AI：你可能在忙或不在电脑前，先做不受影响的工作，不替你决定，并在最后汇报里列出这个问题。',
};

/**
 * An answer to the model's question as the questions and what was replied,
 * with what the model was told when the user declined or nobody responded.
 */
export function QueryReplyCard({ view }: { view: QueryReplyView }) {
  return (
    <article className="agent-user-message agent-query-reply" data-outcome={view.outcome}>
      <small>{HEADINGS[view.outcome]}</small>
      <ol>
        {view.entries.map((entry, index) => (
          <li key={index}>
            {entry.question ? (
              <span className="agent-query-reply-question">{entry.question}</span>
            ) : null}
            <span className="agent-query-reply-answer">
              {entry.custom ? <span className="agent-query-reply-custom">自定义</span> : null}
              {entry.answer}
            </span>
          </li>
        ))}
      </ol>
      {NOTES[view.outcome] ? <p className="agent-query-reply-note">{NOTES[view.outcome]}</p> : null}
    </article>
  );
}
