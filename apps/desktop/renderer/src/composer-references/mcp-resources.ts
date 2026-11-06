import type {
  AgentAttachmentDraft,
  McpResourceProblem,
  McpResourceView,
} from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

/** The resources of a conversation's ready MCP servers, and the servers that could not list theirs. */
export interface McpResources {
  resources: readonly McpResourceView[];
  problems: readonly McpResourceProblem[];
}

const NONE: McpResources = { resources: [], problems: [] };

/** Read once per opening of the "@" list, through the conversation's own instances of the servers. */
export function useMcpResources(open: boolean, session: string | undefined): McpResources {
  const [listed, setListed] = useState<McpResources>(NONE);
  useEffect(() => {
    if (!open || !session) return;
    let live = true;
    // A desktop without this call still lists everything else.
    void Promise.resolve()
      .then(() => window.aura.v1.listMcpResources({ session_id: session }))
      .then(
        (result) => {
          if (live && result.ok) setListed(result.value);
        },
        () => undefined,
      );
    return () => {
      live = false;
      setListed(NONE);
    };
  }, [open, session]);
  return listed;
}

/** The name a resource is shown and kept under. */
export const resourceName = (resource: McpResourceView): string => resource.title ?? resource.name;

export const resourceKey = (resource: McpResourceView): string =>
  `resource:${resource.server_id}:${resource.uri}`;

/** A chosen resource as the message's attachment: Core reads it when the message is sent. */
export function resourceDraft(resource: McpResourceView, name: string): AgentAttachmentDraft {
  return { kind: 'mcp_resource', server_id: resource.server_id, uri: resource.uri, name };
}
