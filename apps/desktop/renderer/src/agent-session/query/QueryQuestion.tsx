import type { QueryAnswerValue, QueryQuestion, UserQuery } from '@aura/contracts/generated';
import { QueryFile } from './QueryFile';
import { MAX_CUSTOM_REPLY_CHARS, type QuestionDraft } from './query-draft';

function Option({
  checked,
  label,
  multiple,
  name,
  onChange,
}: {
  checked: boolean;
  label: string;
  multiple: boolean;
  name: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="agent-query-option" data-checked={checked || undefined}>
      <input
        checked={checked}
        name={name}
        onChange={(event) => onChange(event.target.checked)}
        type={multiple ? 'checkbox' : 'radio'}
      />
      <span>{label}</span>
    </label>
  );
}

export function QueryQuestionBlock({
  index,
  question,
  query,
  sessionId,
  draft,
  onDraft,
}: {
  index: number;
  question: QueryQuestion;
  query: UserQuery;
  sessionId: string;
  draft: QuestionDraft;
  onDraft: (draft: QuestionDraft) => void;
}) {
  const kind = question.answer_type;
  const name = `${query.query_id}:${question.id}`;
  const multiple = kind.type === 'multi_choice';
  const chosen = (id: string) =>
    !draft.custom && draft.value?.answer === 'choices' && draft.value.ids.includes(id);
  const choose = (value: QueryAnswerValue | undefined) =>
    onDraft({ ...draft, custom: false, value });
  return (
    <fieldset className="agent-query-question">
      <legend>
        <span className="agent-query-index" aria-hidden="true">
          {index + 1}
        </span>
        <span className="agent-query-text">{question.text}</span>
      </legend>
      {kind.type === 'text' ? (
        <textarea
          aria-label={`${question.text}（你的回答）`}
          className="agent-query-textarea"
          maxLength={MAX_CUSTOM_REPLY_CHARS}
          onChange={(event) => choose({ answer: 'text', text: event.target.value })}
          placeholder="用你自己的话回答…"
          value={draft.value?.answer === 'text' ? draft.value.text : ''}
        />
      ) : (
        <div className="agent-query-options">
          {kind.type === 'single_choice' || kind.type === 'multi_choice'
            ? kind.options.map((option) => (
                <Option
                  checked={chosen(option.id)}
                  key={option.id}
                  label={option.label}
                  multiple={multiple}
                  name={name}
                  onChange={(checked) => {
                    const old =
                      !draft.custom && draft.value?.answer === 'choices' ? draft.value.ids : [];
                    choose({
                      answer: 'choices',
                      ids: multiple
                        ? checked
                          ? [...old, option.id]
                          : old.filter((id) => id !== option.id)
                        : [option.id],
                    });
                  }}
                />
              ))
            : null}
          {kind.type === 'confirm'
            ? ([true, false] as const).map((accepted) => (
                <Option
                  checked={
                    !draft.custom &&
                    draft.value?.answer === 'confirm' &&
                    draft.value.accepted === accepted
                  }
                  key={String(accepted)}
                  label={accepted ? '确认' : '不确认'}
                  multiple={false}
                  name={name}
                  onChange={() => choose({ answer: 'confirm', accepted })}
                />
              ))
            : null}
          {kind.type === 'file_or_directory' && !draft.custom ? (
            <QueryFile
              sessionId={sessionId}
              query={query}
              questionId={question.id}
              onSelected={(id) =>
                choose(id ? { answer: 'file_or_directory', file_reference_id: id } : undefined)
              }
            />
          ) : null}
          <Option
            checked={draft.custom}
            label="自定义回复"
            multiple={multiple || kind.type === 'file_or_directory'}
            name={name}
            onChange={(checked) =>
              onDraft({
                ...draft,
                custom: multiple || kind.type === 'file_or_directory' ? checked : true,
              })
            }
          />
        </div>
      )}
      {draft.custom ? (
        <textarea
          aria-label={`${question.text}（自定义回复）`}
          autoFocus
          className="agent-query-textarea"
          maxLength={MAX_CUSTOM_REPLY_CHARS}
          onChange={(event) => onDraft({ ...draft, customText: event.target.value })}
          placeholder="用你自己的话回复…"
          value={draft.customText}
        />
      ) : null}
    </fieldset>
  );
}
