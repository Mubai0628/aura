import { Fragment, type ReactNode } from 'react';

import { programPieces } from './program-mentions';
import { useKnownProgram } from './program-tokens';
import { ProgramIcon } from './ProgramIcon';
import './program-blocks.css';

/**
 * A program chosen with "@" in a sent message: the composer's block, the program's icon before
 * its name. The words stay as written until the program is known, and for a name no program has.
 */
export function MessageProgram({ name, children }: { name: string; children: ReactNode }) {
  const appId = useKnownProgram(name);
  if (appId === undefined) return <>{children}</>;
  return (
    <span className="message-program-block">
      <ProgramIcon appId={appId} size={14} />
      {name}
    </span>
  );
}

/** Plain words, each `@"name"` in them drawn as a [`MessageProgram`]. */
export function ProgramWords({ text }: { text: string }) {
  return (
    <>
      {programPieces(text).map((piece, index) =>
        typeof piece === 'string' ? (
          <Fragment key={index}>{piece}</Fragment>
        ) : (
          <MessageProgram key={index} name={piece.name}>
            {piece.text}
          </MessageProgram>
        ),
      )}
    </>
  );
}
