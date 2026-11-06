import { useCallback, useState } from 'react';
import type { ComputerControlledV1, ComputerPipStatus } from '@aura/contracts/generated';

import { userMessage } from '../agent-session/user-message';
import { usePolling } from '../ui/usePolling';
import { useReadOrder } from '../ui/useReadOrder';
import { computerControl } from './bridge';
import { type GiveBackConversation, tellModel } from './give-back';

/** How often an open conversation reads which programs its model is controlling. */
export const CONTROLLING_POLL_MS = 2_000;

const NOT_STOPPED = '没有停止，请重试。';
const NOT_GIVEN_BACK = '没有交还，请重试。';

/** `*` while 全部停止 is out. */
type Pending = string;

export interface Controlling {
  apps: readonly ComputerControlledV1[];
  pip?: ComputerPipStatus | undefined;
  pending: Pending | undefined;
  /** The program whose desktop is being given back. */
  givingBack: string | undefined;
  failure: string | undefined;
  /** Stops one program for the conversation, or each one it is controlling now. */
  stop: (app?: string) => Promise<void>;
  /** Gives the desktop the user took over back, then tells the model. */
  giveBack: (app: ComputerControlledV1) => Promise<void>;
}

/**
 * What the collapsed activity card says of the desktop: the programs the model still controls,
 * and whether the user took one over, which stays listed until the desktop is given back or the
 * conversation's next turn runs.
 */
export function desktopState(apps: readonly ComputerControlledV1[]): {
  controlled: number;
  takenOver: boolean;
} {
  const controlled = apps.filter((app) => app.taken_over !== true).length;
  return { controlled, takenOver: controlled < apps.length };
}

/** The programs the conversation's model is controlling now, read while it is open. */
export function useComputerControlling(
  sessionId: string | undefined,
  conversation: GiveBackConversation = {},
): Controlling {
  // Kept with the conversation they belong to: another conversation shows nothing until read.
  const [read, setRead] = useState<{
    session: string;
    apps: ComputerControlledV1[];
    pip: ComputerPipStatus | undefined;
  }>();
  const [pending, setPending] = useState<Pending>();
  const [givingBack, setGivingBack] = useState<string>();
  const [failed, setFailed] = useState<{ session: string; text: string }>();
  const reads = useReadOrder();
  const poll = useCallback(
    async (live: () => boolean = () => true) => {
      if (sessionId === undefined) return;
      const asked = reads.begin();
      const result = await computerControl()?.controlling({ session_id: sessionId });
      if (live() && reads.accept(asked) && result?.ok)
        setRead({
          session: sessionId,
          apps: result.value.apps,
          pip: result.value.pip ?? undefined,
        });
    },
    [sessionId, reads],
  );
  usePolling(
    sessionId === undefined || computerControl() === undefined ? undefined : poll,
    CONTROLLING_POLL_MS,
  );
  const fail = (session: string, text: string | undefined) =>
    setFailed(text === undefined ? undefined : { session, text });
  /** What Core answered is newer than any read still out. */
  const answered = (
    session: string,
    value: { apps: ComputerControlledV1[]; pip?: ComputerPipStatus | null },
  ) => {
    reads.settle();
    setRead({ session, apps: value.apps, pip: value.pip ?? undefined });
  };
  const stop = async (app?: string) => {
    if (sessionId === undefined) return;
    setPending(app ?? '*');
    try {
      const result = await computerControl()?.stop({
        session_id: sessionId,
        ...(app === undefined ? {} : { app_id: app }),
      });
      if (result === undefined) fail(sessionId, NOT_STOPPED);
      else if (!result.ok) fail(sessionId, userMessage(result.error, NOT_STOPPED));
      else {
        fail(sessionId, undefined);
        answered(sessionId, result.value);
      }
    } catch {
      fail(sessionId, NOT_STOPPED);
    } finally {
      setPending(undefined);
    }
  };
  const giveBack = async (app: ComputerControlledV1) => {
    if (sessionId === undefined) return;
    setGivingBack(app.app_id);
    try {
      const result = await computerControl()?.giveBack({ session_id: sessionId });
      if (result === undefined) fail(sessionId, NOT_GIVEN_BACK);
      else if (!result.ok) fail(sessionId, userMessage(result.error, NOT_GIVEN_BACK));
      else {
        answered(sessionId, result.value);
        fail(sessionId, await tellModel(sessionId, app, conversation));
      }
    } catch {
      fail(sessionId, NOT_GIVEN_BACK);
    } finally {
      setGivingBack(undefined);
    }
  };
  const apps = read !== undefined && read.session === sessionId ? read.apps : [];
  return {
    apps,
    pip: read?.session === sessionId ? read?.pip : undefined,
    pending,
    givingBack,
    failure: failed !== undefined && failed.session === sessionId ? failed.text : undefined,
    stop,
    giveBack,
  };
}
