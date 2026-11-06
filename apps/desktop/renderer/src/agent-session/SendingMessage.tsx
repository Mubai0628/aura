import { ProgramWords } from '../composer-references/MessagePrograms';
import { QuotedWords } from './quote-reply/QuotedWords';
import type { Sending } from './sending';

/** The message being sent, in the place the conversation will show it. */
export function SendingMessage({ sending }: { sending: Sending }) {
  return (
    <article className="agent-user-message agent-sending" aria-busy="true">
      {sending.text.trim() ? (
        <QuotedWords
          className="agent-message-text"
          text={sending.text}
          words={(text) => <ProgramWords text={text} />}
        />
      ) : null}
      {sending.files.length ? (
        <p className="agent-sending-files">{sending.files.join('、')}</p>
      ) : null}
      <small className="agent-sending-state">发送中</small>
    </article>
  );
}
