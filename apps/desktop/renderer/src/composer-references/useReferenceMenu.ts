import type {
  AgentAttachmentDraft,
  AgentMessageReference,
  McpResourceView,
  PathSearchMatch,
  SkillView,
} from '@aura/contracts/generated';
import type { KeyboardEvent, RefObject } from 'react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useKeptDraft } from '../agent-session/kept-draft';
import { userMessage } from '../agent-session/user-message';
import { atAllowedAt, atQuery, typedAt, withPath, withProgram, withoutToken } from './at-token';
import {
  CaptureUnavailable,
  capture,
  pageOptions,
  terminalOptions,
  type CaptureOption,
} from './captures';
import { resourceDraft, resourceKey, resourceName, useMcpResources } from './mcp-resources';
import { matchesQuery } from './pinyin';
import { knowPrograms } from './program-tokens';
import { usePrograms } from './programs';

/** The workspace the list looks in, and its conversation once there is one. */
export interface ReferenceScope {
  workspace: string;
  session?: string | undefined;
}

/** Another conversation of the workspace a message may refer to. */
export interface ConversationOption {
  id: string;
  title: string;
}

/** The composer's Skills: the ones usable now, the ones chosen, and how one is chosen. */
export interface ReferenceSkills {
  load: () => Promise<readonly SkillView[]>;
  chosen: readonly { id: string }[];
  add: (skill: SkillView) => void;
}

export interface ReferenceOption {
  key: string;
  kind: 'file' | 'folder' | 'skill' | 'session' | 'terminal' | 'page' | 'resource' | 'program';
  /**
   * The workspace path or the program's name, as it is written after "@"; a Skill's,
   * conversation's, tab's or resource's name.
   */
  label: string;
  /** A terminal's shell, a page's address or a resource's server. */
  detail?: string;
  /** The conversation's id. */
  session?: string;
  skill?: SkillView;
  capture?: CaptureOption;
  resource?: McpResourceView;
  /** The program's id in the computer's catalog. */
  appId?: string;
  /** A pinned Skill already at the start of every request: noted, not chosen. */
  loaded?: boolean;
}

/** A conversation, terminal, page or MCP resource chosen for the message, shown as a chip. */
export interface ChosenReference {
  key: string;
  kind: 'session' | 'terminal' | 'page' | 'resource';
  title: string;
  /** The conversation's id. */
  session?: string | undefined;
  capture?: CaptureOption | undefined;
  resource?: McpResourceView | undefined;
}

const NOT_CHOSEN: ChosenReference[] = [];

/** `start` is where the typed "@" is. */
interface MenuState {
  start: number;
  query: string;
  highlight: number;
}

interface Found {
  query: string;
  options: ReferenceOption[];
  error?: string;
}

/** How many paths the list shows at once, and how many of each other kind. */
const SHOWN = 30;
const SHOWN_OF_KIND = 10;
/** The most one message refers to. */
const MOST_REFERRED = 20;
/** The most a search returns: the desktop's limit for one. */
const SEARCHED = 100;

const lastName = (path: string) => path.slice(path.lastIndexOf('/') + 1);

/** The paths the list offers: not ones Git ignores, and not links to elsewhere. */
function offered(matches: readonly PathSearchMatch[]): ReferenceOption[] {
  return matches
    .filter((match) => !match.ignored && match.kind !== 'reparse_point')
    .map((match) => ({
      key: `path:${match.path}`,
      kind: match.kind === 'directory' ? 'folder' : 'file',
      label: match.path,
    }));
}

/**
 * The list behind a typed "@": the workspace's files and folders, the usable Skills, its other
 * conversations, this conversation's terminals and browser pages, the resources of its ready MCP
 * servers and, while computer control is on, the computer's programs, as the words after it are
 * typed, Chinese names by their pinyin initials too. A path or program picked is written as
 * "@path" or "@\"name\"" in place of what was typed; a Skill picked is chosen for the turn
 * as "+ → Skill" chooses it; anything else picked becomes a chip sent with the message, and what
 * was typed goes. A terminal or page is read when the message is sent, a resource by Core. The
 * chips are kept for the conversation `draftKey` names while Aura runs.
 */
export function useReferenceMenu({
  textarea,
  draft,
  setDraft,
  scope,
  conversations = [],
  skills,
  draftKey,
}: {
  textarea: RefObject<HTMLTextAreaElement | null>;
  draft: string;
  setDraft: (text: string) => void;
  scope: ReferenceScope | undefined;
  conversations?: readonly ConversationOption[] | undefined;
  skills?: ReferenceSkills | undefined;
  draftKey?: string | undefined;
}) {
  const [menu, setMenu] = useState<MenuState>();
  const [chosen, setChosen] = useKeptDraft('references', draftKey, NOT_CHOSEN);
  const [captureError, setCaptureError] = useState<string>();
  const [found, setFound] = useState<Found>();
  // The workspace's first entries, most recently used first, read once per opening: names in
  // Chinese among them are found by pinyin.
  const [all, setAll] = useState<ReferenceOption[]>();
  const [usable, setUsable] = useState<readonly SkillView[]>([]);
  const typed = useRef<number | null>(null);
  const caretAfter = useRef<number | null>(null);

  useEffect(() => {
    const element = textarea.current;
    if (!element) return;
    const beforeInput = (event: InputEvent) => {
      const at = element.selectionStart;
      typed.current =
        typedAt(event) && at === element.selectionEnd && atAllowedAt(element.value, at) ? at : null;
    };
    element.addEventListener('beforeinput', beforeInput);
    return () => element.removeEventListener('beforeinput', beforeInput);
  }, [textarea]);

  // A pick changes the draft; the caret goes after the path written.
  useEffect(() => {
    const caret = caretAfter.current;
    if (caret === null) return;
    caretAfter.current = null;
    textarea.current?.focus();
    textarea.current?.setSelectionRange(caret, caret);
  }, [draft, textarea]);

  const open = menu !== undefined;
  const query = menu?.query;
  const session = scope?.session;
  const workspace = scope?.workspace;
  useEffect(() => {
    if (query === undefined || !workspace) return;
    let live = true;
    // Searched once typing pauses, not on every key.
    const timer = setTimeout(
      () => {
        void window.aura.v1
          .findWorkspacePaths({
            ...(session ? { session_id: session } : {}),
            workspace_id: workspace,
            query,
            limit: SEARCHED,
          })
          .then(
            (result) => {
              if (!live) return;
              setFound(
                result.ok
                  ? { query, options: offered(result.value.matches) }
                  : {
                      query,
                      options: [],
                      error: userMessage(result.error, '没有找到，请重试。'),
                    },
              );
            },
            (reason: unknown) => {
              if (live)
                setFound({ query, options: [], error: userMessage(reason, '没有找到，请重试。') });
            },
          );
      },
      query ? 150 : 0,
    );
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, session, workspace]);

  useEffect(() => {
    if (!open || !workspace) return;
    let live = true;
    void window.aura.v1
      .findWorkspacePaths({
        ...(session ? { session_id: session } : {}),
        workspace_id: workspace,
        query: '',
        limit: SEARCHED,
      })
      .then(
        (result) => {
          if (live && result.ok) setAll(offered(result.value.matches));
        },
        () => undefined,
      );
    return () => {
      live = false;
      setAll(undefined);
    };
  }, [open, session, workspace]);

  // The usable Skills, read once per opening.
  const loadSkills = skills?.load;
  useEffect(() => {
    if (!open || !loadSkills) return;
    let live = true;
    void loadSkills().then(
      (list) => {
        if (live) setUsable(list);
      },
      () => undefined,
    );
    return () => {
      live = false;
      setUsable([]);
    };
  }, [open, loadSkills]);

  const mcp = useMcpResources(open, session);
  const programs = usePrograms(open);

  // This conversation's terminals and pages as they are when the list opens.
  const captures = useMemo(
    () => (open ? [...terminalOptions(session), ...pageOptions(session)] : []),
    [open, session],
  );

  const options: ReferenceOption[] = useMemo(() => {
    if (!menu) return [];
    // The paths found for what is typed now; none while that is still being looked up.
    const direct = found?.query === menu.query ? found.options : undefined;
    const keys = new Set((direct ?? []).map((option) => option.key));
    // Names in Chinese, found by the first letters of their pinyin.
    const byPinyin =
      direct && menu.query
        ? (all ?? []).filter(
            (option) => !keys.has(option.key) && matchesQuery(lastName(option.label), menu.query),
          )
        : [];
    const paths = [...(direct ?? []), ...byPinyin];
    const taken = new Set(chosen.map((one) => one.key));
    const fits = (label: string, detail = '') =>
      matchesQuery(label, menu.query) || (detail !== '' && matchesQuery(detail, menu.query));
    const skillOptions: ReferenceOption[] = usable
      .filter(
        (skill) =>
          !skills?.chosen.some((one) => one.id === skill.id) && fits(skill.name ?? skill.relative),
      )
      .slice(0, SHOWN_OF_KIND)
      .map((skill) => ({
        key: `skill:${skill.id}`,
        kind: 'skill',
        label: skill.name ?? skill.relative,
        skill,
        loaded: Boolean(skill.pinned && !skill.pinned_left_out),
      }));
    const others: ReferenceOption[] = conversations
      .filter(
        (conversation) =>
          !taken.has(`session:${conversation.id}`) && matchesQuery(conversation.title, menu.query),
      )
      .slice(0, SHOWN_OF_KIND)
      .map((conversation) => ({
        key: `session:${conversation.id}`,
        kind: 'session',
        label: conversation.title,
        session: conversation.id,
      }));
    const of = (kind: CaptureOption['kind']): ReferenceOption[] =>
      captures
        .filter(
          (one) =>
            one.kind === kind &&
            !taken.has(`${kind}:${one.id}`) &&
            fits(one.title, kind === 'page' ? one.detail : ''),
        )
        .slice(0, SHOWN_OF_KIND)
        .map((one) => ({
          key: `${kind}:${one.id}`,
          kind,
          label: one.title,
          detail: one.detail,
          capture: one,
        }));
    const resources: ReferenceOption[] = mcp.resources
      .filter(
        (resource) =>
          !taken.has(resourceKey(resource)) && fits(resourceName(resource), resource.server_name),
      )
      .slice(0, SHOWN_OF_KIND)
      .map((resource) => ({
        key: resourceKey(resource),
        kind: 'resource',
        label: resourceName(resource),
        detail: resource.server_name,
        resource,
      }));
    const programOptions: ReferenceOption[] = programs
      .filter((program) => fits(program.name))
      .slice(0, SHOWN_OF_KIND)
      .map((program) => ({
        key: `program:${program.app_id}`,
        kind: 'program',
        label: program.name,
        appId: program.app_id,
      }));
    return [
      ...paths.slice(0, SHOWN),
      ...skillOptions,
      ...others,
      ...of('terminal'),
      ...of('page'),
      ...resources,
      ...programOptions,
    ];
  }, [menu, found, all, usable, skills, conversations, captures, mcp, programs, chosen]);

  /** After every edit or caret move: open on a typed "@", follow the token, or close. */
  const sync = (value: string, caret: number) => {
    const start = typed.current;
    typed.current = null;
    if (start !== null && value[start] === '@' && caret === start + 1) {
      setMenu({ start, query: '', highlight: 0 });
      return;
    }
    if (menu === undefined) return;
    const next = atQuery(value, menu.start, caret);
    if (next === undefined) setMenu(undefined);
    else if (next !== menu.query) setMenu({ ...menu, query: next, highlight: 0 });
  };

  const pick = (option: ReferenceOption) => {
    if (!menu || option.loaded) return;
    const caret = textarea.current?.selectionStart ?? draft.length;
    setMenu(undefined);
    // A program is written by its name in quotes, as a path is written, and the model reads it
    // from the words; the composer shows it as a block with the program's icon.
    const next =
      option.kind === 'program'
        ? withProgram(draft, menu.start, caret, option.label)
        : option.kind === 'file' || option.kind === 'folder'
          ? withPath(draft, menu.start, caret, option.label)
          : withoutToken(draft, menu.start, caret);
    if (option.kind === 'program' && option.appId !== undefined)
      knowPrograms([{ app_id: option.appId, name: option.label }]);
    if (option.skill) skills?.add(option.skill);
    else if (
      option.kind === 'session' ||
      option.kind === 'terminal' ||
      option.kind === 'page' ||
      option.kind === 'resource'
    ) {
      const { kind, key, label, session, capture, resource } = option;
      setCaptureError(undefined);
      setChosen((current) =>
        current.length >= MOST_REFERRED || current.some((one) => one.key === key)
          ? current
          : [...current, { key, kind, title: label, session, capture, resource }],
      );
    }
    caretAfter.current = next.caret;
    setDraft(next.value);
  };

  /** True when the key belonged to the list and must not also send or type. */
  const keyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!menu || event.nativeEvent.isComposing) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = options.length;
      if (count)
        setMenu({
          ...menu,
          highlight: (menu.highlight + (event.key === 'ArrowDown' ? 1 : count - 1)) % count,
        });
      return true;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      setMenu(undefined);
      return true;
    }
    if (event.key === 'Enter' || event.key === 'Tab') {
      const option = options[menu.highlight];
      // Nothing to choose: the typed text is ordinary text, and Enter sends it as typed.
      if (option === undefined) {
        setMenu(undefined);
        return false;
      }
      event.preventDefault();
      pick(option);
      return true;
    }
    return false;
  };

  return {
    menu,
    options,
    /** The conversations, terminals, pages and resources chosen for the message, as chips. */
    chosen,
    /** The servers that could not list their resources now. */
    resourceProblems: mcp.problems,
    captureError,
    unchoose: (key: string) => {
      setCaptureError(undefined);
      setChosen((current) => current.filter((one) => one.key !== key));
    },
    clearChosen: () => {
      setCaptureError(undefined);
      setChosen([]);
    },
    /** What the message refers to besides the paths in its words. */
    chosenReferences: (): AgentMessageReference[] =>
      chosen.flatMap((one): AgentMessageReference[] =>
        one.session ? [{ kind: 'session', session_id: one.session }] : [],
      ),
    /**
     * The chosen terminals and pages as they are now and the chosen resources, as the message's
     * attachments; undefined, with the reason shown, when one cannot be read.
     */
    captured: async (): Promise<AgentAttachmentDraft[] | undefined> => {
      const sent = chosen.filter((one) => one.capture || one.resource);
      if (!sent.length) return [];
      if (!session) return undefined;
      try {
        const drafts = await Promise.all(
          sent.map(async (one) =>
            one.capture ? capture(session, one.capture) : resourceDraft(one.resource!, one.title),
          ),
        );
        setCaptureError(undefined);
        return drafts;
      } catch (reason) {
        setCaptureError(
          reason instanceof CaptureUnavailable ? reason.message : '读不到引用的内容，请重试。',
        );
        return undefined;
      }
    },
    searching: menu !== undefined && Boolean(scope) && found?.query !== menu.query,
    error: menu && found?.query === menu.query ? found.error : undefined,
    available: Boolean(scope),
    sync,
    pick,
    keyDown,
    hover: (index: number) => setMenu((current) => current && { ...current, highlight: index }),
    close: () => setMenu(undefined),
  };
}
