// @ts-nocheck
/* eslint-disable */
/** Generated from Rust route/schema facts using AJV standalone. Do not edit. */
/* AJV runtime helpers, and fast-deep-equal (Copyright (c) 2017 Evgeny Poberezkin).
The MIT License (MIT)

Copyright (c) 2015-2021 Evgeny Poberezkin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

*/
'use strict';
export const validateGitRequest = validate52;
const schema19 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.runtime.get' },
        value: { $ref: '#/$defs/EmptyToolInput' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.workspace.read' },
        value: { $ref: '#/$defs/GitWorkspaceReadRequestV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.installation.open' },
        value: { $ref: '#/$defs/EmptyToolInput' },
      },
    },
  ],
  $defs: {
    EmptyToolInput: { additionalProperties: false, type: 'object' },
    GitRuntimeStatusV1: {
      additionalProperties: false,
      properties: {
        executable: { type: ['string', 'null'] },
        installation_url: { type: 'string' },
        message: { type: 'string' },
        next_step: { type: 'string' },
        state: { $ref: '#/$defs/GitRuntimeStateV1' },
        version: { type: ['string', 'null'] },
      },
      required: ['state', 'message', 'next_step', 'installation_url'],
      type: 'object',
    },
    GitRuntimeStateV1: { enum: ['available', 'missing', 'too_old', 'invalid'], type: 'string' },
    GitWorkspaceReadRequestV1: {
      additionalProperties: false,
      properties: {
        query: { $ref: '#/$defs/GitReadInputV1' },
        session_id: { $ref: '#/$defs/SessionId' },
        workspace_id: { $ref: '#/$defs/WorkspaceId' },
      },
      required: ['workspace_id', 'session_id', 'query'],
      type: 'object',
    },
    GitReadInputV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitStatusInput' },
            kind: { const: 'status', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitDiffInput' },
            kind: { const: 'diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitLogInput' },
            kind: { const: 'log', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          description: "Desktop review only. The model's git.diff schema remains GitDiffInput.",
          properties: {
            input: { $ref: '#/$defs/GitUntrackedDiffInputV1' },
            kind: { const: 'untracked_diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          description:
            "Desktop review only: the working tree against HEAD or against where HEAD left a\nbranch. The model's git.diff schema remains GitDiffInput.",
          properties: {
            input: { $ref: '#/$defs/GitReviewDiffInputV1' },
            kind: { const: 'review_diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
      ],
    },
    GitStatusInput: {
      additionalProperties: false,
      properties: {
        include_branches: { default: false, type: 'boolean' },
        include_untracked: { default: true, type: 'boolean' },
        limit: { default: 200, maximum: 2000, minimum: 1, type: 'integer' },
        offset: { default: 0, minimum: 0, type: 'integer', maximum: 4294967295 },
      },
      type: 'object',
    },
    GitDiffInput: {
      additionalProperties: false,
      properties: {
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        target: { $ref: '#/$defs/GitDiffTargetV1', default: { kind: 'unstaged' } },
      },
      type: 'object',
    },
    GitDiffTargetV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: { kind: { const: 'unstaged', type: 'string' } },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { kind: { const: 'staged', type: 'string' } },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { commit: { type: 'string' }, kind: { const: 'commit', type: 'string' } },
          required: ['kind', 'commit'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            from: { type: 'string' },
            kind: { const: 'range', type: 'string' },
            to: { type: 'string' },
          },
          required: ['kind', 'from', 'to'],
          type: 'object',
        },
      ],
    },
    GitLogInput: {
      additionalProperties: false,
      properties: {
        commit: { default: null, type: ['string', 'null'] },
        limit: { default: 20, maximum: 200, minimum: 1, type: 'integer' },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        start: { default: null, type: ['string', 'null'] },
      },
      type: 'object',
    },
    GitUntrackedDiffInputV1: {
      additionalProperties: false,
      properties: {
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        path: { type: 'string' },
      },
      required: ['path'],
      type: 'object',
    },
    GitReviewDiffInputV1: {
      additionalProperties: false,
      description:
        "The review page's comparison. Untracked files are not part of it: status lists them and\n`UntrackedDiff` reads them one by one.",
      properties: {
        base: {
          default: null,
          description:
            'None: the uncommitted changes, staged or not, against HEAD (the empty tree before the\nfirst commit). A branch: the working tree against the commit where HEAD left it.',
          type: ['string', 'null'],
        },
        ignore_whitespace: {
          default: false,
          description:
            'Lines that differ only in whitespace are not changes; files with nothing else drop out.',
          type: 'boolean',
        },
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        stats_only: {
          default: false,
          description:
            "Only the files and their line counts, with an empty patch. The review page lists\nthe files this way and reads each file's lines when it is shown.",
          type: 'boolean',
        },
      },
      type: 'object',
    },
    SessionId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    WorkspaceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    GitReadResultV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'status', type: 'string' },
            result: { $ref: '#/$defs/GitStatusResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'diff', type: 'string' },
            result: { $ref: '#/$defs/GitDiffResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'log', type: 'string' },
            result: { $ref: '#/$defs/GitLogResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
      ],
    },
    GitStatusResultV1: {
      additionalProperties: false,
      properties: {
        ahead: { minimum: 0, type: 'integer', maximum: 4294967295 },
        behind: { minimum: 0, type: 'integer', maximum: 4294967295 },
        branch: { type: ['string', 'null'] },
        branches: { items: { $ref: '#/$defs/GitBranchV1' }, type: 'array' },
        conflicts: { items: { type: 'string' }, type: 'array' },
        detached: { type: 'boolean' },
        entries: { items: { $ref: '#/$defs/GitChangeV1' }, type: 'array' },
        head: { type: ['string', 'null'] },
        next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 4294967295 },
        operation: { type: ['string', 'null'] },
        submodule_contents_checked: {
          description: 'Submodule contents are deliberately not traversed or executed.',
          type: 'boolean',
        },
        total_entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
        truncated: { type: 'boolean' },
        upstream: { type: ['string', 'null'] },
      },
      required: [
        'detached',
        'ahead',
        'behind',
        'conflicts',
        'entries',
        'branches',
        'total_entries',
        'truncated',
        'submodule_contents_checked',
      ],
      type: 'object',
    },
    GitBranchV1: {
      additionalProperties: false,
      properties: {
        commit: { type: 'string' },
        name: { type: 'string' },
        remote: { type: 'boolean' },
      },
      required: ['name', 'commit', 'remote'],
      type: 'object',
    },
    GitChangeV1: {
      additionalProperties: false,
      properties: {
        comparison_note: {
          description:
            'Present only when an attribute selects an external converter suppressed by isolated reads.',
          type: ['string', 'null'],
        },
        conflict: { type: 'boolean' },
        index_status: { type: 'string' },
        original_path: { type: ['string', 'null'] },
        path: { type: 'string' },
        submodule: { type: 'boolean' },
        untracked: { type: 'boolean' },
        worktree_status: { type: 'string' },
      },
      required: ['path', 'index_status', 'worktree_status', 'conflict', 'untracked', 'submodule'],
      type: 'object',
    },
    GitDiffResultV1: {
      additionalProperties: false,
      properties: {
        files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
        next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        offset_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        patch: { type: 'string' },
        total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        truncated: { type: 'boolean' },
        warnings: { items: { type: 'string' }, type: 'array' },
      },
      required: ['warnings', 'files', 'patch', 'offset_bytes', 'total_bytes', 'truncated'],
      type: 'object',
    },
    GitFileStatV1: {
      additionalProperties: false,
      properties: {
        added: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        binary: { type: 'boolean' },
        comparison_note: { type: ['string', 'null'] },
        deleted: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        original_path: { type: ['string', 'null'] },
        path: { type: 'string' },
      },
      required: ['path', 'binary'],
      type: 'object',
    },
    GitLogResultV1: {
      additionalProperties: false,
      properties: {
        commits: { items: { $ref: '#/$defs/GitCommitV1' }, type: 'array' },
        truncated: { type: 'boolean' },
      },
      required: ['commits', 'truncated'],
      type: 'object',
    },
    GitCommitV1: {
      additionalProperties: false,
      properties: {
        author: { type: 'string' },
        authored_at: { type: 'string' },
        body: { type: 'string' },
        commit: { type: 'string' },
        files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
        parents: { items: { type: 'string' }, type: 'array' },
        subject: { type: 'string' },
      },
      required: ['commit', 'parents', 'author', 'authored_at', 'subject', 'body', 'files'],
      type: 'object',
    },
    GitInstallationResultV1: {
      additionalProperties: false,
      properties: { opened: { type: 'boolean' } },
      required: ['opened'],
      type: 'object',
    },
    GitCommitInput: {
      additionalProperties: false,
      properties: {
        message: {
          description: 'Required non-empty commit message. No amend, editor or hook bypass option.',
          maxLength: 65536,
          minLength: 1,
          type: 'string',
        },
        paths: {
          default: [],
          description:
            'Literal workspace-relative files to stage and commit; excludes unrelated staged files.',
          items: { type: 'string' },
          maxItems: 200,
          type: 'array',
        },
        staged_only: {
          default: false,
          description: 'Commit the existing index only. Mutually exclusive with non-empty paths.',
          type: 'boolean',
        },
      },
      required: ['message'],
      type: 'object',
    },
    GitBranchInput: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'create', type: 'string' },
            name: { maxLength: 1024, minLength: 1, type: 'string' },
            start: { default: null, type: ['string', 'null'] },
            switch: { default: false, type: 'boolean' },
          },
          required: ['kind', 'name'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'switch', type: 'string' },
            name: { maxLength: 1024, minLength: 1, type: 'string' },
          },
          required: ['kind', 'name'],
          type: 'object',
        },
      ],
      type: 'object',
    },
    GitCommitResultV1: {
      additionalProperties: false,
      properties: {
        branch: { type: ['string', 'null'] },
        commit: { $ref: '#/$defs/GitCommitV1' },
        execution_notice: { type: 'string' },
        status: {
          $ref: '#/$defs/GitStatusResultV1',
          description:
            'Bounded post-operation status, including remaining untracked and staged changes.',
        },
      },
      required: ['commit', 'status', 'execution_notice'],
      type: 'object',
    },
    GitBranchResultV1: {
      additionalProperties: false,
      properties: {
        branch: { type: 'string' },
        created: { type: 'boolean' },
        execution_notice: { type: 'string' },
        head: { type: ['string', 'null'] },
        previous_branch: { type: ['string', 'null'] },
        previous_head: { type: ['string', 'null'] },
        status: { $ref: '#/$defs/GitStatusResultV1' },
        switched: { type: 'boolean' },
      },
      required: ['branch', 'created', 'switched', 'status', 'execution_notice'],
      type: 'object',
    },
    GitSyncInput: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'fetch', type: 'string' },
            remote: { default: null, type: ['string', 'null'] },
          },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            branch: { default: null, type: ['string', 'null'] },
            kind: { const: 'pull', type: 'string' },
            remote: { default: null, type: ['string', 'null'] },
          },
          required: ['kind'],
          type: 'object',
        },
      ],
      type: 'object',
    },
    GitPushInput: {
      additionalProperties: false,
      properties: {
        branch: { default: null, type: ['string', 'null'] },
        force_with_lease: {
          default: null,
          description:
            'Full expected remote object ID. An exact lease, never a bare force or tracking-ref lease.',
          type: ['string', 'null'],
        },
        remote: { default: null, type: ['string', 'null'] },
        remote_branch: { default: null, type: ['string', 'null'] },
        set_upstream: { default: false, type: 'boolean' },
      },
      type: 'object',
    },
    GitRemoteResultV1: {
      additionalProperties: false,
      properties: {
        execution_notice: { type: 'string' },
        kind: { type: 'string' },
        local_branch: { type: ['string', 'null'] },
        refs: { items: { $ref: '#/$defs/GitRefUpdateV1' }, type: 'array' },
        remote: { type: 'string' },
        remote_branch: { type: ['string', 'null'] },
        remote_url: {
          description: 'Normalized display and consent URL; has no userinfo, query or fragment.',
          type: 'string',
        },
        status: { $ref: '#/$defs/GitStatusResultV1' },
      },
      required: ['kind', 'remote', 'remote_url', 'refs', 'status', 'execution_notice'],
      type: 'object',
    },
    GitRefUpdateV1: {
      additionalProperties: false,
      properties: {
        after: { type: ['string', 'null'] },
        before: { type: ['string', 'null'] },
        reference: { type: 'string' },
        result: { type: 'string' },
      },
      required: ['reference', 'result'],
      type: 'object',
    },
  },
};
const schema20 = { additionalProperties: false, type: 'object' };
function validate53(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate53.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      for (const key0 in data) {
        validate53.errors = [
          {
            instancePath,
            schemaPath: '#/additionalProperties',
            keyword: 'additionalProperties',
            params: { additionalProperty: key0 },
            message: 'must NOT have additional properties',
          },
        ];
        return false;
        break;
      }
    } else {
      validate53.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate53.errors = vErrors;
  return errors === 0;
}
validate53.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema21 = {
  additionalProperties: false,
  properties: {
    query: { $ref: '#/$defs/GitReadInputV1' },
    session_id: { $ref: '#/$defs/SessionId' },
    workspace_id: { $ref: '#/$defs/WorkspaceId' },
  },
  required: ['workspace_id', 'session_id', 'query'],
  type: 'object',
};
const schema22 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: {
        input: { $ref: '#/$defs/GitStatusInput' },
        kind: { const: 'status', type: 'string' },
      },
      required: ['kind', 'input'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        input: { $ref: '#/$defs/GitDiffInput' },
        kind: { const: 'diff', type: 'string' },
      },
      required: ['kind', 'input'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        input: { $ref: '#/$defs/GitLogInput' },
        kind: { const: 'log', type: 'string' },
      },
      required: ['kind', 'input'],
      type: 'object',
    },
    {
      additionalProperties: false,
      description: "Desktop review only. The model's git.diff schema remains GitDiffInput.",
      properties: {
        input: { $ref: '#/$defs/GitUntrackedDiffInputV1' },
        kind: { const: 'untracked_diff', type: 'string' },
      },
      required: ['kind', 'input'],
      type: 'object',
    },
    {
      additionalProperties: false,
      description:
        "Desktop review only: the working tree against HEAD or against where HEAD left a\nbranch. The model's git.diff schema remains GitDiffInput.",
      properties: {
        input: { $ref: '#/$defs/GitReviewDiffInputV1' },
        kind: { const: 'review_diff', type: 'string' },
      },
      required: ['kind', 'input'],
      type: 'object',
    },
  ],
};
const schema23 = {
  additionalProperties: false,
  properties: {
    include_branches: { default: false, type: 'boolean' },
    include_untracked: { default: true, type: 'boolean' },
    limit: { default: 200, maximum: 2000, minimum: 1, type: 'integer' },
    offset: { default: 0, minimum: 0, type: 'integer', maximum: 4294967295 },
  },
  type: 'object',
};
function validate57(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate57.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(
          key0 === 'include_branches' ||
          key0 === 'include_untracked' ||
          key0 === 'limit' ||
          key0 === 'offset'
        )) {
          validate57.errors = [
            {
              instancePath,
              schemaPath: '#/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            },
          ];
          return false;
          break;
        }
      }
      if (_errs1 === errors) {
        if (data.include_branches !== undefined) {
          const _errs2 = errors;
          if (typeof data.include_branches !== 'boolean') {
            validate57.errors = [
              {
                instancePath: instancePath + '/include_branches',
                schemaPath: '#/properties/include_branches/type',
                keyword: 'type',
                params: { type: 'boolean' },
                message: 'must be boolean',
              },
            ];
            return false;
          }
          var valid0 = _errs2 === errors;
        } else {
          var valid0 = true;
        }
        if (valid0) {
          if (data.include_untracked !== undefined) {
            const _errs4 = errors;
            if (typeof data.include_untracked !== 'boolean') {
              validate57.errors = [
                {
                  instancePath: instancePath + '/include_untracked',
                  schemaPath: '#/properties/include_untracked/type',
                  keyword: 'type',
                  params: { type: 'boolean' },
                  message: 'must be boolean',
                },
              ];
              return false;
            }
            var valid0 = _errs4 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.limit !== undefined) {
              let data2 = data.limit;
              const _errs6 = errors;
              if (!(typeof data2 == 'number' && !(data2 % 1) && !isNaN(data2) && isFinite(data2))) {
                validate57.errors = [
                  {
                    instancePath: instancePath + '/limit',
                    schemaPath: '#/properties/limit/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs6) {
                if (typeof data2 == 'number' && isFinite(data2)) {
                  if (data2 > 2000 || isNaN(data2)) {
                    validate57.errors = [
                      {
                        instancePath: instancePath + '/limit',
                        schemaPath: '#/properties/limit/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 2000 },
                        message: 'must be <= 2000',
                      },
                    ];
                    return false;
                  } else {
                    if (data2 < 1 || isNaN(data2)) {
                      validate57.errors = [
                        {
                          instancePath: instancePath + '/limit',
                          schemaPath: '#/properties/limit/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 1 },
                          message: 'must be >= 1',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.offset !== undefined) {
                let data3 = data.offset;
                const _errs8 = errors;
                if (!(
                  typeof data3 == 'number' &&
                  !(data3 % 1) &&
                  !isNaN(data3) &&
                  isFinite(data3)
                )) {
                  validate57.errors = [
                    {
                      instancePath: instancePath + '/offset',
                      schemaPath: '#/properties/offset/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs8) {
                  if (typeof data3 == 'number' && isFinite(data3)) {
                    if (data3 > 4294967295 || isNaN(data3)) {
                      validate57.errors = [
                        {
                          instancePath: instancePath + '/offset',
                          schemaPath: '#/properties/offset/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 4294967295 },
                          message: 'must be <= 4294967295',
                        },
                      ];
                      return false;
                    } else {
                      if (data3 < 0 || isNaN(data3)) {
                        validate57.errors = [
                          {
                            instancePath: instancePath + '/offset',
                            schemaPath: '#/properties/offset/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate57.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate57.errors = vErrors;
  return errors === 0;
}
validate57.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema24 = {
  additionalProperties: false,
  properties: {
    max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
    offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
    paths: { default: [], items: { type: 'string' }, type: 'array' },
    target: { $ref: '#/$defs/GitDiffTargetV1', default: { kind: 'unstaged' } },
  },
  type: 'object',
};
const schema25 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: { kind: { const: 'unstaged', type: 'string' } },
      required: ['kind'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: { kind: { const: 'staged', type: 'string' } },
      required: ['kind'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: { commit: { type: 'string' }, kind: { const: 'commit', type: 'string' } },
      required: ['kind', 'commit'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        from: { type: 'string' },
        kind: { const: 'range', type: 'string' },
        to: { type: 'string' },
      },
      required: ['kind', 'from', 'to'],
      type: 'object',
    },
  ],
};
function validate60(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate60.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.kind === undefined && (missing0 = 'kind')) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'kind')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.kind !== undefined) {
            let data0 = data.kind;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('unstaged' !== data0) {
              const err3 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'unstaged' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (data.kind === undefined && (missing1 = 'kind')) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'kind')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.kind !== undefined) {
            let data1 = data.kind;
            if (typeof data1 !== 'string') {
              const err7 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/1/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err7];
              } else {
                vErrors.push(err7);
              }
              errors++;
            }
            if ('staged' !== data1) {
              const err8 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/1/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'staged' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs11 = errors;
    if (errors === _errs11) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.kind === undefined && (missing2 = 'kind')) ||
          (data.commit === undefined && (missing2 = 'commit'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs13 = errors;
          for (const key2 in data) {
            if (!(key2 === 'commit' || key2 === 'kind')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs13 === errors) {
            if (data.commit !== undefined) {
              const _errs14 = errors;
              if (typeof data.commit !== 'string') {
                const err12 = {
                  instancePath: instancePath + '/commit',
                  schemaPath: '#/oneOf/2/properties/commit/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err12];
                } else {
                  vErrors.push(err12);
                }
                errors++;
              }
              var valid3 = _errs14 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.kind !== undefined) {
                let data3 = data.kind;
                const _errs16 = errors;
                if (typeof data3 !== 'string') {
                  const err13 = {
                    instancePath: instancePath + '/kind',
                    schemaPath: '#/oneOf/2/properties/kind/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err13];
                  } else {
                    vErrors.push(err13);
                  }
                  errors++;
                }
                if ('commit' !== data3) {
                  const err14 = {
                    instancePath: instancePath + '/kind',
                    schemaPath: '#/oneOf/2/properties/kind/const',
                    keyword: 'const',
                    params: { allowedValue: 'commit' },
                    message: 'must be equal to constant',
                  };
                  if (vErrors === null) {
                    vErrors = [err14];
                  } else {
                    vErrors.push(err14);
                  }
                  errors++;
                }
                var valid3 = _errs16 === errors;
              } else {
                var valid3 = true;
              }
            }
          }
        }
      } else {
        const err15 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err15];
        } else {
          vErrors.push(err15);
        }
        errors++;
      }
    }
    var _valid0 = _errs11 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
      const _errs18 = errors;
      if (errors === _errs18) {
        if (data && typeof data == 'object' && !Array.isArray(data)) {
          let missing3;
          if (
            (data.kind === undefined && (missing3 = 'kind')) ||
            (data.from === undefined && (missing3 = 'from')) ||
            (data.to === undefined && (missing3 = 'to'))
          ) {
            const err16 = {
              instancePath,
              schemaPath: '#/oneOf/3/required',
              keyword: 'required',
              params: { missingProperty: missing3 },
              message: "must have required property '" + missing3 + "'",
            };
            if (vErrors === null) {
              vErrors = [err16];
            } else {
              vErrors.push(err16);
            }
            errors++;
          } else {
            const _errs20 = errors;
            for (const key3 in data) {
              if (!(key3 === 'from' || key3 === 'kind' || key3 === 'to')) {
                const err17 = {
                  instancePath,
                  schemaPath: '#/oneOf/3/additionalProperties',
                  keyword: 'additionalProperties',
                  params: { additionalProperty: key3 },
                  message: 'must NOT have additional properties',
                };
                if (vErrors === null) {
                  vErrors = [err17];
                } else {
                  vErrors.push(err17);
                }
                errors++;
                break;
              }
            }
            if (_errs20 === errors) {
              if (data.from !== undefined) {
                const _errs21 = errors;
                if (typeof data.from !== 'string') {
                  const err18 = {
                    instancePath: instancePath + '/from',
                    schemaPath: '#/oneOf/3/properties/from/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err18];
                  } else {
                    vErrors.push(err18);
                  }
                  errors++;
                }
                var valid4 = _errs21 === errors;
              } else {
                var valid4 = true;
              }
              if (valid4) {
                if (data.kind !== undefined) {
                  let data5 = data.kind;
                  const _errs23 = errors;
                  if (typeof data5 !== 'string') {
                    const err19 = {
                      instancePath: instancePath + '/kind',
                      schemaPath: '#/oneOf/3/properties/kind/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    };
                    if (vErrors === null) {
                      vErrors = [err19];
                    } else {
                      vErrors.push(err19);
                    }
                    errors++;
                  }
                  if ('range' !== data5) {
                    const err20 = {
                      instancePath: instancePath + '/kind',
                      schemaPath: '#/oneOf/3/properties/kind/const',
                      keyword: 'const',
                      params: { allowedValue: 'range' },
                      message: 'must be equal to constant',
                    };
                    if (vErrors === null) {
                      vErrors = [err20];
                    } else {
                      vErrors.push(err20);
                    }
                    errors++;
                  }
                  var valid4 = _errs23 === errors;
                } else {
                  var valid4 = true;
                }
                if (valid4) {
                  if (data.to !== undefined) {
                    const _errs25 = errors;
                    if (typeof data.to !== 'string') {
                      const err21 = {
                        instancePath: instancePath + '/to',
                        schemaPath: '#/oneOf/3/properties/to/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      };
                      if (vErrors === null) {
                        vErrors = [err21];
                      } else {
                        vErrors.push(err21);
                      }
                      errors++;
                    }
                    var valid4 = _errs25 === errors;
                  } else {
                    var valid4 = true;
                  }
                }
              }
            }
          }
        } else {
          const err22 = {
            instancePath,
            schemaPath: '#/oneOf/3/type',
            keyword: 'type',
            params: { type: 'object' },
            message: 'must be object',
          };
          if (vErrors === null) {
            vErrors = [err22];
          } else {
            vErrors.push(err22);
          }
          errors++;
        }
      }
      var _valid0 = _errs18 === errors;
      if (_valid0 && valid0) {
        valid0 = false;
        passing0 = [passing0, 3];
      } else {
        if (_valid0) {
          valid0 = true;
          passing0 = 3;
          if (props0 !== true) {
            props0 = true;
          }
        }
      }
    }
  }
  if (!valid0) {
    const err23 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err23];
    } else {
      vErrors.push(err23);
    }
    errors++;
    validate60.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate60.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate60.evaluated = { dynamicProps: true, dynamicItems: false };
function validate59(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate59.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(
          key0 === 'max_bytes' ||
          key0 === 'offset_bytes' ||
          key0 === 'paths' ||
          key0 === 'target'
        )) {
          validate59.errors = [
            {
              instancePath,
              schemaPath: '#/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            },
          ];
          return false;
          break;
        }
      }
      if (_errs1 === errors) {
        if (data.max_bytes !== undefined) {
          let data0 = data.max_bytes;
          const _errs2 = errors;
          if (!(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0))) {
            validate59.errors = [
              {
                instancePath: instancePath + '/max_bytes',
                schemaPath: '#/properties/max_bytes/type',
                keyword: 'type',
                params: { type: 'integer' },
                message: 'must be integer',
              },
            ];
            return false;
          }
          if (errors === _errs2) {
            if (typeof data0 == 'number' && isFinite(data0)) {
              if (data0 > 65536 || isNaN(data0)) {
                validate59.errors = [
                  {
                    instancePath: instancePath + '/max_bytes',
                    schemaPath: '#/properties/max_bytes/maximum',
                    keyword: 'maximum',
                    params: { comparison: '<=', limit: 65536 },
                    message: 'must be <= 65536',
                  },
                ];
                return false;
              } else {
                if (data0 < 1 || isNaN(data0)) {
                  validate59.errors = [
                    {
                      instancePath: instancePath + '/max_bytes',
                      schemaPath: '#/properties/max_bytes/minimum',
                      keyword: 'minimum',
                      params: { comparison: '>=', limit: 1 },
                      message: 'must be >= 1',
                    },
                  ];
                  return false;
                }
              }
            }
          }
          var valid0 = _errs2 === errors;
        } else {
          var valid0 = true;
        }
        if (valid0) {
          if (data.offset_bytes !== undefined) {
            let data1 = data.offset_bytes;
            const _errs4 = errors;
            if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
              validate59.errors = [
                {
                  instancePath: instancePath + '/offset_bytes',
                  schemaPath: '#/properties/offset_bytes/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs4) {
              if (typeof data1 == 'number' && isFinite(data1)) {
                if (data1 > 9007199254740991 || isNaN(data1)) {
                  validate59.errors = [
                    {
                      instancePath: instancePath + '/offset_bytes',
                      schemaPath: '#/properties/offset_bytes/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 9007199254740991 },
                      message: 'must be <= 9007199254740991',
                    },
                  ];
                  return false;
                } else {
                  if (data1 < 0 || isNaN(data1)) {
                    validate59.errors = [
                      {
                        instancePath: instancePath + '/offset_bytes',
                        schemaPath: '#/properties/offset_bytes/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs4 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.paths !== undefined) {
              let data2 = data.paths;
              const _errs6 = errors;
              if (errors === _errs6) {
                if (Array.isArray(data2)) {
                  var valid1 = true;
                  const len0 = data2.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs8 = errors;
                    if (typeof data2[i0] !== 'string') {
                      validate59.errors = [
                        {
                          instancePath: instancePath + '/paths/' + i0,
                          schemaPath: '#/properties/paths/items/type',
                          keyword: 'type',
                          params: { type: 'string' },
                          message: 'must be string',
                        },
                      ];
                      return false;
                    }
                    var valid1 = _errs8 === errors;
                    if (!valid1) {
                      break;
                    }
                  }
                } else {
                  validate59.errors = [
                    {
                      instancePath: instancePath + '/paths',
                      schemaPath: '#/properties/paths/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.target !== undefined) {
                const _errs10 = errors;
                if (
                  !validate60(data.target, {
                    instancePath: instancePath + '/target',
                    parentData: data,
                    parentDataProperty: 'target',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate60.errors : vErrors.concat(validate60.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs10 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate59.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate59.errors = vErrors;
  return errors === 0;
}
validate59.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema26 = {
  additionalProperties: false,
  properties: {
    commit: { default: null, type: ['string', 'null'] },
    limit: { default: 20, maximum: 200, minimum: 1, type: 'integer' },
    paths: { default: [], items: { type: 'string' }, type: 'array' },
    start: { default: null, type: ['string', 'null'] },
  },
  type: 'object',
};
function validate63(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate63.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(key0 === 'commit' || key0 === 'limit' || key0 === 'paths' || key0 === 'start')) {
          validate63.errors = [
            {
              instancePath,
              schemaPath: '#/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            },
          ];
          return false;
          break;
        }
      }
      if (_errs1 === errors) {
        if (data.commit !== undefined) {
          let data0 = data.commit;
          const _errs2 = errors;
          if (typeof data0 !== 'string' && data0 !== null) {
            validate63.errors = [
              {
                instancePath: instancePath + '/commit',
                schemaPath: '#/properties/commit/type',
                keyword: 'type',
                params: { type: schema26.properties.commit.type },
                message: 'must be string,null',
              },
            ];
            return false;
          }
          var valid0 = _errs2 === errors;
        } else {
          var valid0 = true;
        }
        if (valid0) {
          if (data.limit !== undefined) {
            let data1 = data.limit;
            const _errs4 = errors;
            if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
              validate63.errors = [
                {
                  instancePath: instancePath + '/limit',
                  schemaPath: '#/properties/limit/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs4) {
              if (typeof data1 == 'number' && isFinite(data1)) {
                if (data1 > 200 || isNaN(data1)) {
                  validate63.errors = [
                    {
                      instancePath: instancePath + '/limit',
                      schemaPath: '#/properties/limit/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 200 },
                      message: 'must be <= 200',
                    },
                  ];
                  return false;
                } else {
                  if (data1 < 1 || isNaN(data1)) {
                    validate63.errors = [
                      {
                        instancePath: instancePath + '/limit',
                        schemaPath: '#/properties/limit/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 1 },
                        message: 'must be >= 1',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs4 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.paths !== undefined) {
              let data2 = data.paths;
              const _errs6 = errors;
              if (errors === _errs6) {
                if (Array.isArray(data2)) {
                  var valid1 = true;
                  const len0 = data2.length;
                  for (let i0 = 0; i0 < len0; i0++) {
                    const _errs8 = errors;
                    if (typeof data2[i0] !== 'string') {
                      validate63.errors = [
                        {
                          instancePath: instancePath + '/paths/' + i0,
                          schemaPath: '#/properties/paths/items/type',
                          keyword: 'type',
                          params: { type: 'string' },
                          message: 'must be string',
                        },
                      ];
                      return false;
                    }
                    var valid1 = _errs8 === errors;
                    if (!valid1) {
                      break;
                    }
                  }
                } else {
                  validate63.errors = [
                    {
                      instancePath: instancePath + '/paths',
                      schemaPath: '#/properties/paths/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.start !== undefined) {
                let data4 = data.start;
                const _errs10 = errors;
                if (typeof data4 !== 'string' && data4 !== null) {
                  validate63.errors = [
                    {
                      instancePath: instancePath + '/start',
                      schemaPath: '#/properties/start/type',
                      keyword: 'type',
                      params: { type: schema26.properties.start.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs10 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate63.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate63.errors = vErrors;
  return errors === 0;
}
validate63.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema27 = {
  additionalProperties: false,
  properties: {
    max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
    offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
    path: { type: 'string' },
  },
  required: ['path'],
  type: 'object',
};
function validate65(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate65.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.path === undefined && (missing0 = 'path')) {
        validate65.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'max_bytes' || key0 === 'offset_bytes' || key0 === 'path')) {
            validate65.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.max_bytes !== undefined) {
            let data0 = data.max_bytes;
            const _errs2 = errors;
            if (!(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0))) {
              validate65.errors = [
                {
                  instancePath: instancePath + '/max_bytes',
                  schemaPath: '#/properties/max_bytes/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 65536 || isNaN(data0)) {
                  validate65.errors = [
                    {
                      instancePath: instancePath + '/max_bytes',
                      schemaPath: '#/properties/max_bytes/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 65536 },
                      message: 'must be <= 65536',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 1 || isNaN(data0)) {
                    validate65.errors = [
                      {
                        instancePath: instancePath + '/max_bytes',
                        schemaPath: '#/properties/max_bytes/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 1 },
                        message: 'must be >= 1',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.offset_bytes !== undefined) {
              let data1 = data.offset_bytes;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate65.errors = [
                  {
                    instancePath: instancePath + '/offset_bytes',
                    schemaPath: '#/properties/offset_bytes/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 9007199254740991 || isNaN(data1)) {
                    validate65.errors = [
                      {
                        instancePath: instancePath + '/offset_bytes',
                        schemaPath: '#/properties/offset_bytes/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 9007199254740991 },
                        message: 'must be <= 9007199254740991',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate65.errors = [
                        {
                          instancePath: instancePath + '/offset_bytes',
                          schemaPath: '#/properties/offset_bytes/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.path !== undefined) {
                const _errs6 = errors;
                if (typeof data.path !== 'string') {
                  validate65.errors = [
                    {
                      instancePath: instancePath + '/path',
                      schemaPath: '#/properties/path/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate65.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate65.errors = vErrors;
  return errors === 0;
}
validate65.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema28 = {
  additionalProperties: false,
  description:
    "The review page's comparison. Untracked files are not part of it: status lists them and\n`UntrackedDiff` reads them one by one.",
  properties: {
    base: {
      default: null,
      description:
        'None: the uncommitted changes, staged or not, against HEAD (the empty tree before the\nfirst commit). A branch: the working tree against the commit where HEAD left it.',
      type: ['string', 'null'],
    },
    ignore_whitespace: {
      default: false,
      description:
        'Lines that differ only in whitespace are not changes; files with nothing else drop out.',
      type: 'boolean',
    },
    max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
    offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
    paths: { default: [], items: { type: 'string' }, type: 'array' },
    stats_only: {
      default: false,
      description:
        "Only the files and their line counts, with an empty patch. The review page lists\nthe files this way and reads each file's lines when it is shown.",
      type: 'boolean',
    },
  },
  type: 'object',
};
function validate67(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate67.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(
          key0 === 'base' ||
          key0 === 'ignore_whitespace' ||
          key0 === 'max_bytes' ||
          key0 === 'offset_bytes' ||
          key0 === 'paths' ||
          key0 === 'stats_only'
        )) {
          validate67.errors = [
            {
              instancePath,
              schemaPath: '#/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            },
          ];
          return false;
          break;
        }
      }
      if (_errs1 === errors) {
        if (data.base !== undefined) {
          let data0 = data.base;
          const _errs2 = errors;
          if (typeof data0 !== 'string' && data0 !== null) {
            validate67.errors = [
              {
                instancePath: instancePath + '/base',
                schemaPath: '#/properties/base/type',
                keyword: 'type',
                params: { type: schema28.properties.base.type },
                message: 'must be string,null',
              },
            ];
            return false;
          }
          var valid0 = _errs2 === errors;
        } else {
          var valid0 = true;
        }
        if (valid0) {
          if (data.ignore_whitespace !== undefined) {
            const _errs4 = errors;
            if (typeof data.ignore_whitespace !== 'boolean') {
              validate67.errors = [
                {
                  instancePath: instancePath + '/ignore_whitespace',
                  schemaPath: '#/properties/ignore_whitespace/type',
                  keyword: 'type',
                  params: { type: 'boolean' },
                  message: 'must be boolean',
                },
              ];
              return false;
            }
            var valid0 = _errs4 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.max_bytes !== undefined) {
              let data2 = data.max_bytes;
              const _errs6 = errors;
              if (!(typeof data2 == 'number' && !(data2 % 1) && !isNaN(data2) && isFinite(data2))) {
                validate67.errors = [
                  {
                    instancePath: instancePath + '/max_bytes',
                    schemaPath: '#/properties/max_bytes/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs6) {
                if (typeof data2 == 'number' && isFinite(data2)) {
                  if (data2 > 65536 || isNaN(data2)) {
                    validate67.errors = [
                      {
                        instancePath: instancePath + '/max_bytes',
                        schemaPath: '#/properties/max_bytes/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 65536 },
                        message: 'must be <= 65536',
                      },
                    ];
                    return false;
                  } else {
                    if (data2 < 1 || isNaN(data2)) {
                      validate67.errors = [
                        {
                          instancePath: instancePath + '/max_bytes',
                          schemaPath: '#/properties/max_bytes/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 1 },
                          message: 'must be >= 1',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.offset_bytes !== undefined) {
                let data3 = data.offset_bytes;
                const _errs8 = errors;
                if (!(
                  typeof data3 == 'number' &&
                  !(data3 % 1) &&
                  !isNaN(data3) &&
                  isFinite(data3)
                )) {
                  validate67.errors = [
                    {
                      instancePath: instancePath + '/offset_bytes',
                      schemaPath: '#/properties/offset_bytes/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs8) {
                  if (typeof data3 == 'number' && isFinite(data3)) {
                    if (data3 > 9007199254740991 || isNaN(data3)) {
                      validate67.errors = [
                        {
                          instancePath: instancePath + '/offset_bytes',
                          schemaPath: '#/properties/offset_bytes/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 9007199254740991 },
                          message: 'must be <= 9007199254740991',
                        },
                      ];
                      return false;
                    } else {
                      if (data3 < 0 || isNaN(data3)) {
                        validate67.errors = [
                          {
                            instancePath: instancePath + '/offset_bytes',
                            schemaPath: '#/properties/offset_bytes/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.paths !== undefined) {
                  let data4 = data.paths;
                  const _errs10 = errors;
                  if (errors === _errs10) {
                    if (Array.isArray(data4)) {
                      var valid1 = true;
                      const len0 = data4.length;
                      for (let i0 = 0; i0 < len0; i0++) {
                        const _errs12 = errors;
                        if (typeof data4[i0] !== 'string') {
                          validate67.errors = [
                            {
                              instancePath: instancePath + '/paths/' + i0,
                              schemaPath: '#/properties/paths/items/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid1 = _errs12 === errors;
                        if (!valid1) {
                          break;
                        }
                      }
                    } else {
                      validate67.errors = [
                        {
                          instancePath: instancePath + '/paths',
                          schemaPath: '#/properties/paths/type',
                          keyword: 'type',
                          params: { type: 'array' },
                          message: 'must be array',
                        },
                      ];
                      return false;
                    }
                  }
                  var valid0 = _errs10 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.stats_only !== undefined) {
                    const _errs14 = errors;
                    if (typeof data.stats_only !== 'boolean') {
                      validate67.errors = [
                        {
                          instancePath: instancePath + '/stats_only',
                          schemaPath: '#/properties/stats_only/type',
                          keyword: 'type',
                          params: { type: 'boolean' },
                          message: 'must be boolean',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs14 === errors;
                  } else {
                    var valid0 = true;
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate67.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate67.errors = vErrors;
  return errors === 0;
}
validate67.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate56(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate56.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.kind === undefined && (missing0 = 'kind')) ||
        (data.input === undefined && (missing0 = 'input'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'input' || key0 === 'kind')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.input !== undefined) {
            const _errs4 = errors;
            if (
              !validate57(data.input, {
                instancePath: instancePath + '/input',
                parentData: data,
                parentDataProperty: 'input',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate57.errors : vErrors.concat(validate57.errors);
              errors = vErrors.length;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.kind !== undefined) {
              let data1 = data.kind;
              const _errs5 = errors;
              if (typeof data1 !== 'string') {
                const err2 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/0/properties/kind/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err2];
                } else {
                  vErrors.push(err2);
                }
                errors++;
              }
              if ('status' !== data1) {
                const err3 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/0/properties/kind/const',
                  keyword: 'const',
                  params: { allowedValue: 'status' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err3];
                } else {
                  vErrors.push(err3);
                }
                errors++;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs7 = errors;
  if (errors === _errs7) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.kind === undefined && (missing1 = 'kind')) ||
        (data.input === undefined && (missing1 = 'input'))
      ) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs9 = errors;
        for (const key1 in data) {
          if (!(key1 === 'input' || key1 === 'kind')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs9 === errors) {
          if (data.input !== undefined) {
            const _errs10 = errors;
            if (
              !validate59(data.input, {
                instancePath: instancePath + '/input',
                parentData: data,
                parentDataProperty: 'input',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate59.errors : vErrors.concat(validate59.errors);
              errors = vErrors.length;
            }
            var valid2 = _errs10 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.kind !== undefined) {
              let data3 = data.kind;
              const _errs11 = errors;
              if (typeof data3 !== 'string') {
                const err7 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/1/properties/kind/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err7];
                } else {
                  vErrors.push(err7);
                }
                errors++;
              }
              if ('diff' !== data3) {
                const err8 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/1/properties/kind/const',
                  keyword: 'const',
                  params: { allowedValue: 'diff' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err8];
                } else {
                  vErrors.push(err8);
                }
                errors++;
              }
              var valid2 = _errs11 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs7 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs13 = errors;
    if (errors === _errs13) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.kind === undefined && (missing2 = 'kind')) ||
          (data.input === undefined && (missing2 = 'input'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs15 = errors;
          for (const key2 in data) {
            if (!(key2 === 'input' || key2 === 'kind')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs15 === errors) {
            if (data.input !== undefined) {
              const _errs16 = errors;
              if (
                !validate63(data.input, {
                  instancePath: instancePath + '/input',
                  parentData: data,
                  parentDataProperty: 'input',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate63.errors : vErrors.concat(validate63.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs16 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.kind !== undefined) {
                let data5 = data.kind;
                const _errs17 = errors;
                if (typeof data5 !== 'string') {
                  const err12 = {
                    instancePath: instancePath + '/kind',
                    schemaPath: '#/oneOf/2/properties/kind/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err12];
                  } else {
                    vErrors.push(err12);
                  }
                  errors++;
                }
                if ('log' !== data5) {
                  const err13 = {
                    instancePath: instancePath + '/kind',
                    schemaPath: '#/oneOf/2/properties/kind/const',
                    keyword: 'const',
                    params: { allowedValue: 'log' },
                    message: 'must be equal to constant',
                  };
                  if (vErrors === null) {
                    vErrors = [err13];
                  } else {
                    vErrors.push(err13);
                  }
                  errors++;
                }
                var valid3 = _errs17 === errors;
              } else {
                var valid3 = true;
              }
            }
          }
        }
      } else {
        const err14 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
    var _valid0 = _errs13 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
      const _errs19 = errors;
      if (errors === _errs19) {
        if (data && typeof data == 'object' && !Array.isArray(data)) {
          let missing3;
          if (
            (data.kind === undefined && (missing3 = 'kind')) ||
            (data.input === undefined && (missing3 = 'input'))
          ) {
            const err15 = {
              instancePath,
              schemaPath: '#/oneOf/3/required',
              keyword: 'required',
              params: { missingProperty: missing3 },
              message: "must have required property '" + missing3 + "'",
            };
            if (vErrors === null) {
              vErrors = [err15];
            } else {
              vErrors.push(err15);
            }
            errors++;
          } else {
            const _errs21 = errors;
            for (const key3 in data) {
              if (!(key3 === 'input' || key3 === 'kind')) {
                const err16 = {
                  instancePath,
                  schemaPath: '#/oneOf/3/additionalProperties',
                  keyword: 'additionalProperties',
                  params: { additionalProperty: key3 },
                  message: 'must NOT have additional properties',
                };
                if (vErrors === null) {
                  vErrors = [err16];
                } else {
                  vErrors.push(err16);
                }
                errors++;
                break;
              }
            }
            if (_errs21 === errors) {
              if (data.input !== undefined) {
                const _errs22 = errors;
                if (
                  !validate65(data.input, {
                    instancePath: instancePath + '/input',
                    parentData: data,
                    parentDataProperty: 'input',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate65.errors : vErrors.concat(validate65.errors);
                  errors = vErrors.length;
                }
                var valid4 = _errs22 === errors;
              } else {
                var valid4 = true;
              }
              if (valid4) {
                if (data.kind !== undefined) {
                  let data7 = data.kind;
                  const _errs23 = errors;
                  if (typeof data7 !== 'string') {
                    const err17 = {
                      instancePath: instancePath + '/kind',
                      schemaPath: '#/oneOf/3/properties/kind/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    };
                    if (vErrors === null) {
                      vErrors = [err17];
                    } else {
                      vErrors.push(err17);
                    }
                    errors++;
                  }
                  if ('untracked_diff' !== data7) {
                    const err18 = {
                      instancePath: instancePath + '/kind',
                      schemaPath: '#/oneOf/3/properties/kind/const',
                      keyword: 'const',
                      params: { allowedValue: 'untracked_diff' },
                      message: 'must be equal to constant',
                    };
                    if (vErrors === null) {
                      vErrors = [err18];
                    } else {
                      vErrors.push(err18);
                    }
                    errors++;
                  }
                  var valid4 = _errs23 === errors;
                } else {
                  var valid4 = true;
                }
              }
            }
          }
        } else {
          const err19 = {
            instancePath,
            schemaPath: '#/oneOf/3/type',
            keyword: 'type',
            params: { type: 'object' },
            message: 'must be object',
          };
          if (vErrors === null) {
            vErrors = [err19];
          } else {
            vErrors.push(err19);
          }
          errors++;
        }
      }
      var _valid0 = _errs19 === errors;
      if (_valid0 && valid0) {
        valid0 = false;
        passing0 = [passing0, 3];
      } else {
        if (_valid0) {
          valid0 = true;
          passing0 = 3;
          if (props0 !== true) {
            props0 = true;
          }
        }
        const _errs25 = errors;
        if (errors === _errs25) {
          if (data && typeof data == 'object' && !Array.isArray(data)) {
            let missing4;
            if (
              (data.kind === undefined && (missing4 = 'kind')) ||
              (data.input === undefined && (missing4 = 'input'))
            ) {
              const err20 = {
                instancePath,
                schemaPath: '#/oneOf/4/required',
                keyword: 'required',
                params: { missingProperty: missing4 },
                message: "must have required property '" + missing4 + "'",
              };
              if (vErrors === null) {
                vErrors = [err20];
              } else {
                vErrors.push(err20);
              }
              errors++;
            } else {
              const _errs27 = errors;
              for (const key4 in data) {
                if (!(key4 === 'input' || key4 === 'kind')) {
                  const err21 = {
                    instancePath,
                    schemaPath: '#/oneOf/4/additionalProperties',
                    keyword: 'additionalProperties',
                    params: { additionalProperty: key4 },
                    message: 'must NOT have additional properties',
                  };
                  if (vErrors === null) {
                    vErrors = [err21];
                  } else {
                    vErrors.push(err21);
                  }
                  errors++;
                  break;
                }
              }
              if (_errs27 === errors) {
                if (data.input !== undefined) {
                  const _errs28 = errors;
                  if (
                    !validate67(data.input, {
                      instancePath: instancePath + '/input',
                      parentData: data,
                      parentDataProperty: 'input',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate67.errors : vErrors.concat(validate67.errors);
                    errors = vErrors.length;
                  }
                  var valid5 = _errs28 === errors;
                } else {
                  var valid5 = true;
                }
                if (valid5) {
                  if (data.kind !== undefined) {
                    let data9 = data.kind;
                    const _errs29 = errors;
                    if (typeof data9 !== 'string') {
                      const err22 = {
                        instancePath: instancePath + '/kind',
                        schemaPath: '#/oneOf/4/properties/kind/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      };
                      if (vErrors === null) {
                        vErrors = [err22];
                      } else {
                        vErrors.push(err22);
                      }
                      errors++;
                    }
                    if ('review_diff' !== data9) {
                      const err23 = {
                        instancePath: instancePath + '/kind',
                        schemaPath: '#/oneOf/4/properties/kind/const',
                        keyword: 'const',
                        params: { allowedValue: 'review_diff' },
                        message: 'must be equal to constant',
                      };
                      if (vErrors === null) {
                        vErrors = [err23];
                      } else {
                        vErrors.push(err23);
                      }
                      errors++;
                    }
                    var valid5 = _errs29 === errors;
                  } else {
                    var valid5 = true;
                  }
                }
              }
            }
          } else {
            const err24 = {
              instancePath,
              schemaPath: '#/oneOf/4/type',
              keyword: 'type',
              params: { type: 'object' },
              message: 'must be object',
            };
            if (vErrors === null) {
              vErrors = [err24];
            } else {
              vErrors.push(err24);
            }
            errors++;
          }
        }
        var _valid0 = _errs25 === errors;
        if (_valid0 && valid0) {
          valid0 = false;
          passing0 = [passing0, 4];
        } else {
          if (_valid0) {
            valid0 = true;
            passing0 = 4;
            if (props0 !== true) {
              props0 = true;
            }
          }
        }
      }
    }
  }
  if (!valid0) {
    const err25 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err25];
    } else {
      vErrors.push(err25);
    }
    errors++;
    validate56.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate56.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate56.evaluated = { dynamicProps: true, dynamicItems: false };
const schema29 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
};
const func1 = function ucs2length(str) {
  const len = str.length;
  let length = 0;
  let pos = 0;
  let value;
  while (pos < len) {
    length++;
    value = str.charCodeAt(pos++);
    if (value >= 0xd800 && value <= 0xdbff && pos < len) {
      // high surrogate, and there is a next character
      value = str.charCodeAt(pos);
      if ((value & 0xfc00) === 0xdc00) pos++; // low surrogate
    }
  }
  return length;
};
const pattern3 = new RegExp('^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
function validate70(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate70.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate70.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate70.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern3.test(data)) {
            validate70.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate70.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate70.errors = vErrors;
  return errors === 0;
}
validate70.evaluated = { dynamicProps: false, dynamicItems: false };
const schema30 = {
  type: 'string',
  minLength: 30,
  maxLength: 30,
  pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
};
const pattern4 = new RegExp('^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$', 'u');
function validate72(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate72.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (typeof data === 'string') {
      if (func1(data) > 30) {
        validate72.errors = [
          {
            instancePath,
            schemaPath: '#/maxLength',
            keyword: 'maxLength',
            params: { limit: 30 },
            message: 'must NOT have more than 30 characters',
          },
        ];
        return false;
      } else {
        if (func1(data) < 30) {
          validate72.errors = [
            {
              instancePath,
              schemaPath: '#/minLength',
              keyword: 'minLength',
              params: { limit: 30 },
              message: 'must NOT have fewer than 30 characters',
            },
          ];
          return false;
        } else {
          if (!pattern4.test(data)) {
            validate72.errors = [
              {
                instancePath,
                schemaPath: '#/pattern',
                keyword: 'pattern',
                params: { pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$' },
                message: 'must match pattern "' + '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$' + '"',
              },
            ];
            return false;
          }
        }
      }
    } else {
      validate72.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'string' },
          message: 'must be string',
        },
      ];
      return false;
    }
  }
  validate72.errors = vErrors;
  return errors === 0;
}
validate72.evaluated = { dynamicProps: false, dynamicItems: false };
function validate55(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate55.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.workspace_id === undefined && (missing0 = 'workspace_id')) ||
        (data.session_id === undefined && (missing0 = 'session_id')) ||
        (data.query === undefined && (missing0 = 'query'))
      ) {
        validate55.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'query' || key0 === 'session_id' || key0 === 'workspace_id')) {
            validate55.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.query !== undefined) {
            const _errs2 = errors;
            if (
              !validate56(data.query, {
                instancePath: instancePath + '/query',
                parentData: data,
                parentDataProperty: 'query',
                rootData,
                dynamicAnchors,
              })
            ) {
              vErrors = vErrors === null ? validate56.errors : vErrors.concat(validate56.errors);
              errors = vErrors.length;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.session_id !== undefined) {
              const _errs3 = errors;
              if (
                !validate70(data.session_id, {
                  instancePath: instancePath + '/session_id',
                  parentData: data,
                  parentDataProperty: 'session_id',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate70.errors : vErrors.concat(validate70.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs3 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.workspace_id !== undefined) {
                const _errs4 = errors;
                if (
                  !validate72(data.workspace_id, {
                    instancePath: instancePath + '/workspace_id',
                    parentData: data,
                    parentDataProperty: 'workspace_id',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate72.errors : vErrors.concat(validate72.errors);
                  errors = vErrors.length;
                }
                var valid0 = _errs4 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate55.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate55.errors = vErrors;
  return errors === 0;
}
validate55.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate52(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate52.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.command === undefined && (missing0 = 'command')) ||
        (data.value === undefined && (missing0 = 'value'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/anyOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command' || key0 === 'value')) {
            const err1 = {
              instancePath,
              schemaPath: '#/anyOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.command !== undefined) {
            const _errs4 = errors;
            if ('git.runtime.get' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.runtime.get' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.value !== undefined) {
              const _errs5 = errors;
              if (
                !validate53(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate53.errors : vErrors.concat(validate53.errors);
                errors = vErrors.length;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err3 = {
        instancePath,
        schemaPath: '#/anyOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err3];
      } else {
        vErrors.push(err3);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.command === undefined && (missing1 = 'command')) ||
        (data.value === undefined && (missing1 = 'value'))
      ) {
        const err4 = {
          instancePath,
          schemaPath: '#/anyOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err4];
        } else {
          vErrors.push(err4);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'command' || key1 === 'value')) {
            const err5 = {
              instancePath,
              schemaPath: '#/anyOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err5];
            } else {
              vErrors.push(err5);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.command !== undefined) {
            const _errs9 = errors;
            if ('git.workspace.read' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.workspace.read' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err6];
              } else {
                vErrors.push(err6);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.value !== undefined) {
              const _errs10 = errors;
              if (
                !validate55(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate55.errors : vErrors.concat(validate55.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs10 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err7 = {
        instancePath,
        schemaPath: '#/anyOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err7];
      } else {
        vErrors.push(err7);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs11 = errors;
  if (errors === _errs11) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing2;
      if (
        (data.command === undefined && (missing2 = 'command')) ||
        (data.value === undefined && (missing2 = 'value'))
      ) {
        const err8 = {
          instancePath,
          schemaPath: '#/anyOf/2/required',
          keyword: 'required',
          params: { missingProperty: missing2 },
          message: "must have required property '" + missing2 + "'",
        };
        if (vErrors === null) {
          vErrors = [err8];
        } else {
          vErrors.push(err8);
        }
        errors++;
      } else {
        const _errs13 = errors;
        for (const key2 in data) {
          if (!(key2 === 'command' || key2 === 'value')) {
            const err9 = {
              instancePath,
              schemaPath: '#/anyOf/2/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err9];
            } else {
              vErrors.push(err9);
            }
            errors++;
            break;
          }
        }
        if (_errs13 === errors) {
          if (data.command !== undefined) {
            const _errs14 = errors;
            if ('git.installation.open' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.installation.open' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err10];
              } else {
                vErrors.push(err10);
              }
              errors++;
            }
            var valid3 = _errs14 === errors;
          } else {
            var valid3 = true;
          }
          if (valid3) {
            if (data.value !== undefined) {
              const _errs15 = errors;
              if (
                !validate53(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate53.errors : vErrors.concat(validate53.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
          }
        }
      }
    } else {
      const err11 = {
        instancePath,
        schemaPath: '#/anyOf/2/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err11];
      } else {
        vErrors.push(err11);
      }
      errors++;
    }
  }
  var _valid0 = _errs11 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err12 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err12];
    } else {
      vErrors.push(err12);
    }
    errors++;
    validate52.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate52.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate52.evaluated = { dynamicProps: true, dynamicItems: false };
export const validateGitResponse = validate76;
const schema31 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.runtime.get' },
        value: { $ref: '#/$defs/GitRuntimeStatusV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.workspace.read' },
        value: { $ref: '#/$defs/GitReadResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.installation.open' },
        value: { $ref: '#/$defs/GitInstallationResultV1' },
      },
    },
  ],
  $defs: {
    EmptyToolInput: { additionalProperties: false, type: 'object' },
    GitRuntimeStatusV1: {
      additionalProperties: false,
      properties: {
        executable: { type: ['string', 'null'] },
        installation_url: { type: 'string' },
        message: { type: 'string' },
        next_step: { type: 'string' },
        state: { $ref: '#/$defs/GitRuntimeStateV1' },
        version: { type: ['string', 'null'] },
      },
      required: ['state', 'message', 'next_step', 'installation_url'],
      type: 'object',
    },
    GitRuntimeStateV1: { enum: ['available', 'missing', 'too_old', 'invalid'], type: 'string' },
    GitWorkspaceReadRequestV1: {
      additionalProperties: false,
      properties: {
        query: { $ref: '#/$defs/GitReadInputV1' },
        session_id: { $ref: '#/$defs/SessionId' },
        workspace_id: { $ref: '#/$defs/WorkspaceId' },
      },
      required: ['workspace_id', 'session_id', 'query'],
      type: 'object',
    },
    GitReadInputV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitStatusInput' },
            kind: { const: 'status', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitDiffInput' },
            kind: { const: 'diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitLogInput' },
            kind: { const: 'log', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          description: "Desktop review only. The model's git.diff schema remains GitDiffInput.",
          properties: {
            input: { $ref: '#/$defs/GitUntrackedDiffInputV1' },
            kind: { const: 'untracked_diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          description:
            "Desktop review only: the working tree against HEAD or against where HEAD left a\nbranch. The model's git.diff schema remains GitDiffInput.",
          properties: {
            input: { $ref: '#/$defs/GitReviewDiffInputV1' },
            kind: { const: 'review_diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
      ],
    },
    GitStatusInput: {
      additionalProperties: false,
      properties: {
        include_branches: { default: false, type: 'boolean' },
        include_untracked: { default: true, type: 'boolean' },
        limit: { default: 200, maximum: 2000, minimum: 1, type: 'integer' },
        offset: { default: 0, minimum: 0, type: 'integer', maximum: 4294967295 },
      },
      type: 'object',
    },
    GitDiffInput: {
      additionalProperties: false,
      properties: {
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        target: { $ref: '#/$defs/GitDiffTargetV1', default: { kind: 'unstaged' } },
      },
      type: 'object',
    },
    GitDiffTargetV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: { kind: { const: 'unstaged', type: 'string' } },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { kind: { const: 'staged', type: 'string' } },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { commit: { type: 'string' }, kind: { const: 'commit', type: 'string' } },
          required: ['kind', 'commit'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            from: { type: 'string' },
            kind: { const: 'range', type: 'string' },
            to: { type: 'string' },
          },
          required: ['kind', 'from', 'to'],
          type: 'object',
        },
      ],
    },
    GitLogInput: {
      additionalProperties: false,
      properties: {
        commit: { default: null, type: ['string', 'null'] },
        limit: { default: 20, maximum: 200, minimum: 1, type: 'integer' },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        start: { default: null, type: ['string', 'null'] },
      },
      type: 'object',
    },
    GitUntrackedDiffInputV1: {
      additionalProperties: false,
      properties: {
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        path: { type: 'string' },
      },
      required: ['path'],
      type: 'object',
    },
    GitReviewDiffInputV1: {
      additionalProperties: false,
      description:
        "The review page's comparison. Untracked files are not part of it: status lists them and\n`UntrackedDiff` reads them one by one.",
      properties: {
        base: {
          default: null,
          description:
            'None: the uncommitted changes, staged or not, against HEAD (the empty tree before the\nfirst commit). A branch: the working tree against the commit where HEAD left it.',
          type: ['string', 'null'],
        },
        ignore_whitespace: {
          default: false,
          description:
            'Lines that differ only in whitespace are not changes; files with nothing else drop out.',
          type: 'boolean',
        },
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        stats_only: {
          default: false,
          description:
            "Only the files and their line counts, with an empty patch. The review page lists\nthe files this way and reads each file's lines when it is shown.",
          type: 'boolean',
        },
      },
      type: 'object',
    },
    SessionId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    WorkspaceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    GitReadResultV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'status', type: 'string' },
            result: { $ref: '#/$defs/GitStatusResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'diff', type: 'string' },
            result: { $ref: '#/$defs/GitDiffResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'log', type: 'string' },
            result: { $ref: '#/$defs/GitLogResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
      ],
    },
    GitStatusResultV1: {
      additionalProperties: false,
      properties: {
        ahead: { minimum: 0, type: 'integer', maximum: 4294967295 },
        behind: { minimum: 0, type: 'integer', maximum: 4294967295 },
        branch: { type: ['string', 'null'] },
        branches: { items: { $ref: '#/$defs/GitBranchV1' }, type: 'array' },
        conflicts: { items: { type: 'string' }, type: 'array' },
        detached: { type: 'boolean' },
        entries: { items: { $ref: '#/$defs/GitChangeV1' }, type: 'array' },
        head: { type: ['string', 'null'] },
        next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 4294967295 },
        operation: { type: ['string', 'null'] },
        submodule_contents_checked: {
          description: 'Submodule contents are deliberately not traversed or executed.',
          type: 'boolean',
        },
        total_entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
        truncated: { type: 'boolean' },
        upstream: { type: ['string', 'null'] },
      },
      required: [
        'detached',
        'ahead',
        'behind',
        'conflicts',
        'entries',
        'branches',
        'total_entries',
        'truncated',
        'submodule_contents_checked',
      ],
      type: 'object',
    },
    GitBranchV1: {
      additionalProperties: false,
      properties: {
        commit: { type: 'string' },
        name: { type: 'string' },
        remote: { type: 'boolean' },
      },
      required: ['name', 'commit', 'remote'],
      type: 'object',
    },
    GitChangeV1: {
      additionalProperties: false,
      properties: {
        comparison_note: {
          description:
            'Present only when an attribute selects an external converter suppressed by isolated reads.',
          type: ['string', 'null'],
        },
        conflict: { type: 'boolean' },
        index_status: { type: 'string' },
        original_path: { type: ['string', 'null'] },
        path: { type: 'string' },
        submodule: { type: 'boolean' },
        untracked: { type: 'boolean' },
        worktree_status: { type: 'string' },
      },
      required: ['path', 'index_status', 'worktree_status', 'conflict', 'untracked', 'submodule'],
      type: 'object',
    },
    GitDiffResultV1: {
      additionalProperties: false,
      properties: {
        files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
        next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        offset_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        patch: { type: 'string' },
        total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        truncated: { type: 'boolean' },
        warnings: { items: { type: 'string' }, type: 'array' },
      },
      required: ['warnings', 'files', 'patch', 'offset_bytes', 'total_bytes', 'truncated'],
      type: 'object',
    },
    GitFileStatV1: {
      additionalProperties: false,
      properties: {
        added: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        binary: { type: 'boolean' },
        comparison_note: { type: ['string', 'null'] },
        deleted: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        original_path: { type: ['string', 'null'] },
        path: { type: 'string' },
      },
      required: ['path', 'binary'],
      type: 'object',
    },
    GitLogResultV1: {
      additionalProperties: false,
      properties: {
        commits: { items: { $ref: '#/$defs/GitCommitV1' }, type: 'array' },
        truncated: { type: 'boolean' },
      },
      required: ['commits', 'truncated'],
      type: 'object',
    },
    GitCommitV1: {
      additionalProperties: false,
      properties: {
        author: { type: 'string' },
        authored_at: { type: 'string' },
        body: { type: 'string' },
        commit: { type: 'string' },
        files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
        parents: { items: { type: 'string' }, type: 'array' },
        subject: { type: 'string' },
      },
      required: ['commit', 'parents', 'author', 'authored_at', 'subject', 'body', 'files'],
      type: 'object',
    },
    GitInstallationResultV1: {
      additionalProperties: false,
      properties: { opened: { type: 'boolean' } },
      required: ['opened'],
      type: 'object',
    },
    GitCommitInput: {
      additionalProperties: false,
      properties: {
        message: {
          description: 'Required non-empty commit message. No amend, editor or hook bypass option.',
          maxLength: 65536,
          minLength: 1,
          type: 'string',
        },
        paths: {
          default: [],
          description:
            'Literal workspace-relative files to stage and commit; excludes unrelated staged files.',
          items: { type: 'string' },
          maxItems: 200,
          type: 'array',
        },
        staged_only: {
          default: false,
          description: 'Commit the existing index only. Mutually exclusive with non-empty paths.',
          type: 'boolean',
        },
      },
      required: ['message'],
      type: 'object',
    },
    GitBranchInput: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'create', type: 'string' },
            name: { maxLength: 1024, minLength: 1, type: 'string' },
            start: { default: null, type: ['string', 'null'] },
            switch: { default: false, type: 'boolean' },
          },
          required: ['kind', 'name'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'switch', type: 'string' },
            name: { maxLength: 1024, minLength: 1, type: 'string' },
          },
          required: ['kind', 'name'],
          type: 'object',
        },
      ],
      type: 'object',
    },
    GitCommitResultV1: {
      additionalProperties: false,
      properties: {
        branch: { type: ['string', 'null'] },
        commit: { $ref: '#/$defs/GitCommitV1' },
        execution_notice: { type: 'string' },
        status: {
          $ref: '#/$defs/GitStatusResultV1',
          description:
            'Bounded post-operation status, including remaining untracked and staged changes.',
        },
      },
      required: ['commit', 'status', 'execution_notice'],
      type: 'object',
    },
    GitBranchResultV1: {
      additionalProperties: false,
      properties: {
        branch: { type: 'string' },
        created: { type: 'boolean' },
        execution_notice: { type: 'string' },
        head: { type: ['string', 'null'] },
        previous_branch: { type: ['string', 'null'] },
        previous_head: { type: ['string', 'null'] },
        status: { $ref: '#/$defs/GitStatusResultV1' },
        switched: { type: 'boolean' },
      },
      required: ['branch', 'created', 'switched', 'status', 'execution_notice'],
      type: 'object',
    },
    GitSyncInput: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'fetch', type: 'string' },
            remote: { default: null, type: ['string', 'null'] },
          },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            branch: { default: null, type: ['string', 'null'] },
            kind: { const: 'pull', type: 'string' },
            remote: { default: null, type: ['string', 'null'] },
          },
          required: ['kind'],
          type: 'object',
        },
      ],
      type: 'object',
    },
    GitPushInput: {
      additionalProperties: false,
      properties: {
        branch: { default: null, type: ['string', 'null'] },
        force_with_lease: {
          default: null,
          description:
            'Full expected remote object ID. An exact lease, never a bare force or tracking-ref lease.',
          type: ['string', 'null'],
        },
        remote: { default: null, type: ['string', 'null'] },
        remote_branch: { default: null, type: ['string', 'null'] },
        set_upstream: { default: false, type: 'boolean' },
      },
      type: 'object',
    },
    GitRemoteResultV1: {
      additionalProperties: false,
      properties: {
        execution_notice: { type: 'string' },
        kind: { type: 'string' },
        local_branch: { type: ['string', 'null'] },
        refs: { items: { $ref: '#/$defs/GitRefUpdateV1' }, type: 'array' },
        remote: { type: 'string' },
        remote_branch: { type: ['string', 'null'] },
        remote_url: {
          description: 'Normalized display and consent URL; has no userinfo, query or fragment.',
          type: 'string',
        },
        status: { $ref: '#/$defs/GitStatusResultV1' },
      },
      required: ['kind', 'remote', 'remote_url', 'refs', 'status', 'execution_notice'],
      type: 'object',
    },
    GitRefUpdateV1: {
      additionalProperties: false,
      properties: {
        after: { type: ['string', 'null'] },
        before: { type: ['string', 'null'] },
        reference: { type: 'string' },
        result: { type: 'string' },
      },
      required: ['reference', 'result'],
      type: 'object',
    },
  },
};
const schema32 = {
  additionalProperties: false,
  properties: {
    executable: { type: ['string', 'null'] },
    installation_url: { type: 'string' },
    message: { type: 'string' },
    next_step: { type: 'string' },
    state: { $ref: '#/$defs/GitRuntimeStateV1' },
    version: { type: ['string', 'null'] },
  },
  required: ['state', 'message', 'next_step', 'installation_url'],
  type: 'object',
};
const schema33 = { enum: ['available', 'missing', 'too_old', 'invalid'], type: 'string' };
function validate78(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate78.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (typeof data !== 'string') {
    validate78.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'string' },
        message: 'must be string',
      },
    ];
    return false;
  }
  if (!(data === 'available' || data === 'missing' || data === 'too_old' || data === 'invalid')) {
    validate78.errors = [
      {
        instancePath,
        schemaPath: '#/enum',
        keyword: 'enum',
        params: { allowedValues: schema33.enum },
        message: 'must be equal to one of the allowed values',
      },
    ];
    return false;
  }
  validate78.errors = vErrors;
  return errors === 0;
}
validate78.evaluated = { dynamicProps: false, dynamicItems: false };
function validate77(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate77.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.state === undefined && (missing0 = 'state')) ||
        (data.message === undefined && (missing0 = 'message')) ||
        (data.next_step === undefined && (missing0 = 'next_step')) ||
        (data.installation_url === undefined && (missing0 = 'installation_url'))
      ) {
        validate77.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'executable' ||
            key0 === 'installation_url' ||
            key0 === 'message' ||
            key0 === 'next_step' ||
            key0 === 'state' ||
            key0 === 'version'
          )) {
            validate77.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.executable !== undefined) {
            let data0 = data.executable;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate77.errors = [
                {
                  instancePath: instancePath + '/executable',
                  schemaPath: '#/properties/executable/type',
                  keyword: 'type',
                  params: { type: schema32.properties.executable.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.installation_url !== undefined) {
              const _errs4 = errors;
              if (typeof data.installation_url !== 'string') {
                validate77.errors = [
                  {
                    instancePath: instancePath + '/installation_url',
                    schemaPath: '#/properties/installation_url/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.message !== undefined) {
                const _errs6 = errors;
                if (typeof data.message !== 'string') {
                  validate77.errors = [
                    {
                      instancePath: instancePath + '/message',
                      schemaPath: '#/properties/message/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.next_step !== undefined) {
                  const _errs8 = errors;
                  if (typeof data.next_step !== 'string') {
                    validate77.errors = [
                      {
                        instancePath: instancePath + '/next_step',
                        schemaPath: '#/properties/next_step/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.state !== undefined) {
                    const _errs10 = errors;
                    if (
                      !validate78(data.state, {
                        instancePath: instancePath + '/state',
                        parentData: data,
                        parentDataProperty: 'state',
                        rootData,
                        dynamicAnchors,
                      })
                    ) {
                      vErrors =
                        vErrors === null ? validate78.errors : vErrors.concat(validate78.errors);
                      errors = vErrors.length;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.version !== undefined) {
                      let data5 = data.version;
                      const _errs11 = errors;
                      if (typeof data5 !== 'string' && data5 !== null) {
                        validate77.errors = [
                          {
                            instancePath: instancePath + '/version',
                            schemaPath: '#/properties/version/type',
                            keyword: 'type',
                            params: { type: schema32.properties.version.type },
                            message: 'must be string,null',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs11 === errors;
                    } else {
                      var valid0 = true;
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate77.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate77.errors = vErrors;
  return errors === 0;
}
validate77.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema34 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: {
        kind: { const: 'status', type: 'string' },
        result: { $ref: '#/$defs/GitStatusResultV1' },
      },
      required: ['kind', 'result'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        kind: { const: 'diff', type: 'string' },
        result: { $ref: '#/$defs/GitDiffResultV1' },
      },
      required: ['kind', 'result'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        kind: { const: 'log', type: 'string' },
        result: { $ref: '#/$defs/GitLogResultV1' },
      },
      required: ['kind', 'result'],
      type: 'object',
    },
  ],
};
const schema35 = {
  additionalProperties: false,
  properties: {
    ahead: { minimum: 0, type: 'integer', maximum: 4294967295 },
    behind: { minimum: 0, type: 'integer', maximum: 4294967295 },
    branch: { type: ['string', 'null'] },
    branches: { items: { $ref: '#/$defs/GitBranchV1' }, type: 'array' },
    conflicts: { items: { type: 'string' }, type: 'array' },
    detached: { type: 'boolean' },
    entries: { items: { $ref: '#/$defs/GitChangeV1' }, type: 'array' },
    head: { type: ['string', 'null'] },
    next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 4294967295 },
    operation: { type: ['string', 'null'] },
    submodule_contents_checked: {
      description: 'Submodule contents are deliberately not traversed or executed.',
      type: 'boolean',
    },
    total_entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
    truncated: { type: 'boolean' },
    upstream: { type: ['string', 'null'] },
  },
  required: [
    'detached',
    'ahead',
    'behind',
    'conflicts',
    'entries',
    'branches',
    'total_entries',
    'truncated',
    'submodule_contents_checked',
  ],
  type: 'object',
};
const func5 = Object.prototype.hasOwnProperty;
const schema36 = {
  additionalProperties: false,
  properties: { commit: { type: 'string' }, name: { type: 'string' }, remote: { type: 'boolean' } },
  required: ['name', 'commit', 'remote'],
  type: 'object',
};
function validate83(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate83.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.name === undefined && (missing0 = 'name')) ||
        (data.commit === undefined && (missing0 = 'commit')) ||
        (data.remote === undefined && (missing0 = 'remote'))
      ) {
        validate83.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'commit' || key0 === 'name' || key0 === 'remote')) {
            validate83.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.commit !== undefined) {
            const _errs2 = errors;
            if (typeof data.commit !== 'string') {
              validate83.errors = [
                {
                  instancePath: instancePath + '/commit',
                  schemaPath: '#/properties/commit/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.name !== undefined) {
              const _errs4 = errors;
              if (typeof data.name !== 'string') {
                validate83.errors = [
                  {
                    instancePath: instancePath + '/name',
                    schemaPath: '#/properties/name/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.remote !== undefined) {
                const _errs6 = errors;
                if (typeof data.remote !== 'boolean') {
                  validate83.errors = [
                    {
                      instancePath: instancePath + '/remote',
                      schemaPath: '#/properties/remote/type',
                      keyword: 'type',
                      params: { type: 'boolean' },
                      message: 'must be boolean',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate83.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate83.errors = vErrors;
  return errors === 0;
}
validate83.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema37 = {
  additionalProperties: false,
  properties: {
    comparison_note: {
      description:
        'Present only when an attribute selects an external converter suppressed by isolated reads.',
      type: ['string', 'null'],
    },
    conflict: { type: 'boolean' },
    index_status: { type: 'string' },
    original_path: { type: ['string', 'null'] },
    path: { type: 'string' },
    submodule: { type: 'boolean' },
    untracked: { type: 'boolean' },
    worktree_status: { type: 'string' },
  },
  required: ['path', 'index_status', 'worktree_status', 'conflict', 'untracked', 'submodule'],
  type: 'object',
};
function validate85(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate85.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.path === undefined && (missing0 = 'path')) ||
        (data.index_status === undefined && (missing0 = 'index_status')) ||
        (data.worktree_status === undefined && (missing0 = 'worktree_status')) ||
        (data.conflict === undefined && (missing0 = 'conflict')) ||
        (data.untracked === undefined && (missing0 = 'untracked')) ||
        (data.submodule === undefined && (missing0 = 'submodule'))
      ) {
        validate85.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'comparison_note' ||
            key0 === 'conflict' ||
            key0 === 'index_status' ||
            key0 === 'original_path' ||
            key0 === 'path' ||
            key0 === 'submodule' ||
            key0 === 'untracked' ||
            key0 === 'worktree_status'
          )) {
            validate85.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.comparison_note !== undefined) {
            let data0 = data.comparison_note;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate85.errors = [
                {
                  instancePath: instancePath + '/comparison_note',
                  schemaPath: '#/properties/comparison_note/type',
                  keyword: 'type',
                  params: { type: schema37.properties.comparison_note.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.conflict !== undefined) {
              const _errs4 = errors;
              if (typeof data.conflict !== 'boolean') {
                validate85.errors = [
                  {
                    instancePath: instancePath + '/conflict',
                    schemaPath: '#/properties/conflict/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.index_status !== undefined) {
                const _errs6 = errors;
                if (typeof data.index_status !== 'string') {
                  validate85.errors = [
                    {
                      instancePath: instancePath + '/index_status',
                      schemaPath: '#/properties/index_status/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.original_path !== undefined) {
                  let data3 = data.original_path;
                  const _errs8 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate85.errors = [
                      {
                        instancePath: instancePath + '/original_path',
                        schemaPath: '#/properties/original_path/type',
                        keyword: 'type',
                        params: { type: schema37.properties.original_path.type },
                        message: 'must be string,null',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.path !== undefined) {
                    const _errs10 = errors;
                    if (typeof data.path !== 'string') {
                      validate85.errors = [
                        {
                          instancePath: instancePath + '/path',
                          schemaPath: '#/properties/path/type',
                          keyword: 'type',
                          params: { type: 'string' },
                          message: 'must be string',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.submodule !== undefined) {
                      const _errs12 = errors;
                      if (typeof data.submodule !== 'boolean') {
                        validate85.errors = [
                          {
                            instancePath: instancePath + '/submodule',
                            schemaPath: '#/properties/submodule/type',
                            keyword: 'type',
                            params: { type: 'boolean' },
                            message: 'must be boolean',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs12 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.untracked !== undefined) {
                        const _errs14 = errors;
                        if (typeof data.untracked !== 'boolean') {
                          validate85.errors = [
                            {
                              instancePath: instancePath + '/untracked',
                              schemaPath: '#/properties/untracked/type',
                              keyword: 'type',
                              params: { type: 'boolean' },
                              message: 'must be boolean',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs14 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.worktree_status !== undefined) {
                          const _errs16 = errors;
                          if (typeof data.worktree_status !== 'string') {
                            validate85.errors = [
                              {
                                instancePath: instancePath + '/worktree_status',
                                schemaPath: '#/properties/worktree_status/type',
                                keyword: 'type',
                                params: { type: 'string' },
                                message: 'must be string',
                              },
                            ];
                            return false;
                          }
                          var valid0 = _errs16 === errors;
                        } else {
                          var valid0 = true;
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate85.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate85.errors = vErrors;
  return errors === 0;
}
validate85.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate82(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate82.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.detached === undefined && (missing0 = 'detached')) ||
        (data.ahead === undefined && (missing0 = 'ahead')) ||
        (data.behind === undefined && (missing0 = 'behind')) ||
        (data.conflicts === undefined && (missing0 = 'conflicts')) ||
        (data.entries === undefined && (missing0 = 'entries')) ||
        (data.branches === undefined && (missing0 = 'branches')) ||
        (data.total_entries === undefined && (missing0 = 'total_entries')) ||
        (data.truncated === undefined && (missing0 = 'truncated')) ||
        (data.submodule_contents_checked === undefined && (missing0 = 'submodule_contents_checked'))
      ) {
        validate82.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!func5.call(schema35.properties, key0)) {
            validate82.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.ahead !== undefined) {
            let data0 = data.ahead;
            const _errs2 = errors;
            if (!(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0))) {
              validate82.errors = [
                {
                  instancePath: instancePath + '/ahead',
                  schemaPath: '#/properties/ahead/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 4294967295 || isNaN(data0)) {
                  validate82.errors = [
                    {
                      instancePath: instancePath + '/ahead',
                      schemaPath: '#/properties/ahead/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 4294967295 },
                      message: 'must be <= 4294967295',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate82.errors = [
                      {
                        instancePath: instancePath + '/ahead',
                        schemaPath: '#/properties/ahead/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.behind !== undefined) {
              let data1 = data.behind;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate82.errors = [
                  {
                    instancePath: instancePath + '/behind',
                    schemaPath: '#/properties/behind/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 4294967295 || isNaN(data1)) {
                    validate82.errors = [
                      {
                        instancePath: instancePath + '/behind',
                        schemaPath: '#/properties/behind/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 4294967295 },
                        message: 'must be <= 4294967295',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate82.errors = [
                        {
                          instancePath: instancePath + '/behind',
                          schemaPath: '#/properties/behind/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.branch !== undefined) {
                let data2 = data.branch;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate82.errors = [
                    {
                      instancePath: instancePath + '/branch',
                      schemaPath: '#/properties/branch/type',
                      keyword: 'type',
                      params: { type: schema35.properties.branch.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.branches !== undefined) {
                  let data3 = data.branches;
                  const _errs8 = errors;
                  if (errors === _errs8) {
                    if (Array.isArray(data3)) {
                      var valid1 = true;
                      const len0 = data3.length;
                      for (let i0 = 0; i0 < len0; i0++) {
                        const _errs10 = errors;
                        if (
                          !validate83(data3[i0], {
                            instancePath: instancePath + '/branches/' + i0,
                            parentData: data3,
                            parentDataProperty: i0,
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate83.errors
                              : vErrors.concat(validate83.errors);
                          errors = vErrors.length;
                        }
                        var valid1 = _errs10 === errors;
                        if (!valid1) {
                          break;
                        }
                      }
                    } else {
                      validate82.errors = [
                        {
                          instancePath: instancePath + '/branches',
                          schemaPath: '#/properties/branches/type',
                          keyword: 'type',
                          params: { type: 'array' },
                          message: 'must be array',
                        },
                      ];
                      return false;
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.conflicts !== undefined) {
                    let data5 = data.conflicts;
                    const _errs11 = errors;
                    if (errors === _errs11) {
                      if (Array.isArray(data5)) {
                        var valid2 = true;
                        const len1 = data5.length;
                        for (let i1 = 0; i1 < len1; i1++) {
                          const _errs13 = errors;
                          if (typeof data5[i1] !== 'string') {
                            validate82.errors = [
                              {
                                instancePath: instancePath + '/conflicts/' + i1,
                                schemaPath: '#/properties/conflicts/items/type',
                                keyword: 'type',
                                params: { type: 'string' },
                                message: 'must be string',
                              },
                            ];
                            return false;
                          }
                          var valid2 = _errs13 === errors;
                          if (!valid2) {
                            break;
                          }
                        }
                      } else {
                        validate82.errors = [
                          {
                            instancePath: instancePath + '/conflicts',
                            schemaPath: '#/properties/conflicts/type',
                            keyword: 'type',
                            params: { type: 'array' },
                            message: 'must be array',
                          },
                        ];
                        return false;
                      }
                    }
                    var valid0 = _errs11 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.detached !== undefined) {
                      const _errs15 = errors;
                      if (typeof data.detached !== 'boolean') {
                        validate82.errors = [
                          {
                            instancePath: instancePath + '/detached',
                            schemaPath: '#/properties/detached/type',
                            keyword: 'type',
                            params: { type: 'boolean' },
                            message: 'must be boolean',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs15 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.entries !== undefined) {
                        let data8 = data.entries;
                        const _errs17 = errors;
                        if (errors === _errs17) {
                          if (Array.isArray(data8)) {
                            var valid3 = true;
                            const len2 = data8.length;
                            for (let i2 = 0; i2 < len2; i2++) {
                              const _errs19 = errors;
                              if (
                                !validate85(data8[i2], {
                                  instancePath: instancePath + '/entries/' + i2,
                                  parentData: data8,
                                  parentDataProperty: i2,
                                  rootData,
                                  dynamicAnchors,
                                })
                              ) {
                                vErrors =
                                  vErrors === null
                                    ? validate85.errors
                                    : vErrors.concat(validate85.errors);
                                errors = vErrors.length;
                              }
                              var valid3 = _errs19 === errors;
                              if (!valid3) {
                                break;
                              }
                            }
                          } else {
                            validate82.errors = [
                              {
                                instancePath: instancePath + '/entries',
                                schemaPath: '#/properties/entries/type',
                                keyword: 'type',
                                params: { type: 'array' },
                                message: 'must be array',
                              },
                            ];
                            return false;
                          }
                        }
                        var valid0 = _errs17 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.head !== undefined) {
                          let data10 = data.head;
                          const _errs20 = errors;
                          if (typeof data10 !== 'string' && data10 !== null) {
                            validate82.errors = [
                              {
                                instancePath: instancePath + '/head',
                                schemaPath: '#/properties/head/type',
                                keyword: 'type',
                                params: { type: schema35.properties.head.type },
                                message: 'must be string,null',
                              },
                            ];
                            return false;
                          }
                          var valid0 = _errs20 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.next_offset !== undefined) {
                            let data11 = data.next_offset;
                            const _errs22 = errors;
                            if (
                              !(
                                typeof data11 == 'number' &&
                                !(data11 % 1) &&
                                !isNaN(data11) &&
                                isFinite(data11)
                              ) &&
                              data11 !== null
                            ) {
                              validate82.errors = [
                                {
                                  instancePath: instancePath + '/next_offset',
                                  schemaPath: '#/properties/next_offset/type',
                                  keyword: 'type',
                                  params: { type: schema35.properties.next_offset.type },
                                  message: 'must be integer,null',
                                },
                              ];
                              return false;
                            }
                            if (errors === _errs22) {
                              if (typeof data11 == 'number' && isFinite(data11)) {
                                if (data11 > 4294967295 || isNaN(data11)) {
                                  validate82.errors = [
                                    {
                                      instancePath: instancePath + '/next_offset',
                                      schemaPath: '#/properties/next_offset/maximum',
                                      keyword: 'maximum',
                                      params: { comparison: '<=', limit: 4294967295 },
                                      message: 'must be <= 4294967295',
                                    },
                                  ];
                                  return false;
                                } else {
                                  if (data11 < 0 || isNaN(data11)) {
                                    validate82.errors = [
                                      {
                                        instancePath: instancePath + '/next_offset',
                                        schemaPath: '#/properties/next_offset/minimum',
                                        keyword: 'minimum',
                                        params: { comparison: '>=', limit: 0 },
                                        message: 'must be >= 0',
                                      },
                                    ];
                                    return false;
                                  }
                                }
                              }
                            }
                            var valid0 = _errs22 === errors;
                          } else {
                            var valid0 = true;
                          }
                          if (valid0) {
                            if (data.operation !== undefined) {
                              let data12 = data.operation;
                              const _errs24 = errors;
                              if (typeof data12 !== 'string' && data12 !== null) {
                                validate82.errors = [
                                  {
                                    instancePath: instancePath + '/operation',
                                    schemaPath: '#/properties/operation/type',
                                    keyword: 'type',
                                    params: { type: schema35.properties.operation.type },
                                    message: 'must be string,null',
                                  },
                                ];
                                return false;
                              }
                              var valid0 = _errs24 === errors;
                            } else {
                              var valid0 = true;
                            }
                            if (valid0) {
                              if (data.submodule_contents_checked !== undefined) {
                                const _errs26 = errors;
                                if (typeof data.submodule_contents_checked !== 'boolean') {
                                  validate82.errors = [
                                    {
                                      instancePath: instancePath + '/submodule_contents_checked',
                                      schemaPath: '#/properties/submodule_contents_checked/type',
                                      keyword: 'type',
                                      params: { type: 'boolean' },
                                      message: 'must be boolean',
                                    },
                                  ];
                                  return false;
                                }
                                var valid0 = _errs26 === errors;
                              } else {
                                var valid0 = true;
                              }
                              if (valid0) {
                                if (data.total_entries !== undefined) {
                                  let data14 = data.total_entries;
                                  const _errs28 = errors;
                                  if (!(
                                    typeof data14 == 'number' &&
                                    !(data14 % 1) &&
                                    !isNaN(data14) &&
                                    isFinite(data14)
                                  )) {
                                    validate82.errors = [
                                      {
                                        instancePath: instancePath + '/total_entries',
                                        schemaPath: '#/properties/total_entries/type',
                                        keyword: 'type',
                                        params: { type: 'integer' },
                                        message: 'must be integer',
                                      },
                                    ];
                                    return false;
                                  }
                                  if (errors === _errs28) {
                                    if (typeof data14 == 'number' && isFinite(data14)) {
                                      if (data14 > 4294967295 || isNaN(data14)) {
                                        validate82.errors = [
                                          {
                                            instancePath: instancePath + '/total_entries',
                                            schemaPath: '#/properties/total_entries/maximum',
                                            keyword: 'maximum',
                                            params: { comparison: '<=', limit: 4294967295 },
                                            message: 'must be <= 4294967295',
                                          },
                                        ];
                                        return false;
                                      } else {
                                        if (data14 < 0 || isNaN(data14)) {
                                          validate82.errors = [
                                            {
                                              instancePath: instancePath + '/total_entries',
                                              schemaPath: '#/properties/total_entries/minimum',
                                              keyword: 'minimum',
                                              params: { comparison: '>=', limit: 0 },
                                              message: 'must be >= 0',
                                            },
                                          ];
                                          return false;
                                        }
                                      }
                                    }
                                  }
                                  var valid0 = _errs28 === errors;
                                } else {
                                  var valid0 = true;
                                }
                                if (valid0) {
                                  if (data.truncated !== undefined) {
                                    const _errs30 = errors;
                                    if (typeof data.truncated !== 'boolean') {
                                      validate82.errors = [
                                        {
                                          instancePath: instancePath + '/truncated',
                                          schemaPath: '#/properties/truncated/type',
                                          keyword: 'type',
                                          params: { type: 'boolean' },
                                          message: 'must be boolean',
                                        },
                                      ];
                                      return false;
                                    }
                                    var valid0 = _errs30 === errors;
                                  } else {
                                    var valid0 = true;
                                  }
                                  if (valid0) {
                                    if (data.upstream !== undefined) {
                                      let data16 = data.upstream;
                                      const _errs32 = errors;
                                      if (typeof data16 !== 'string' && data16 !== null) {
                                        validate82.errors = [
                                          {
                                            instancePath: instancePath + '/upstream',
                                            schemaPath: '#/properties/upstream/type',
                                            keyword: 'type',
                                            params: { type: schema35.properties.upstream.type },
                                            message: 'must be string,null',
                                          },
                                        ];
                                        return false;
                                      }
                                      var valid0 = _errs32 === errors;
                                    } else {
                                      var valid0 = true;
                                    }
                                  }
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate82.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate82.errors = vErrors;
  return errors === 0;
}
validate82.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema38 = {
  additionalProperties: false,
  properties: {
    files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
    next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
    offset_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    patch: { type: 'string' },
    total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
    truncated: { type: 'boolean' },
    warnings: { items: { type: 'string' }, type: 'array' },
  },
  required: ['warnings', 'files', 'patch', 'offset_bytes', 'total_bytes', 'truncated'],
  type: 'object',
};
const schema39 = {
  additionalProperties: false,
  properties: {
    added: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
    binary: { type: 'boolean' },
    comparison_note: { type: ['string', 'null'] },
    deleted: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
    original_path: { type: ['string', 'null'] },
    path: { type: 'string' },
  },
  required: ['path', 'binary'],
  type: 'object',
};
function validate89(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate89.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.path === undefined && (missing0 = 'path')) ||
        (data.binary === undefined && (missing0 = 'binary'))
      ) {
        validate89.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'added' ||
            key0 === 'binary' ||
            key0 === 'comparison_note' ||
            key0 === 'deleted' ||
            key0 === 'original_path' ||
            key0 === 'path'
          )) {
            validate89.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.added !== undefined) {
            let data0 = data.added;
            const _errs2 = errors;
            if (
              !(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0)) &&
              data0 !== null
            ) {
              validate89.errors = [
                {
                  instancePath: instancePath + '/added',
                  schemaPath: '#/properties/added/type',
                  keyword: 'type',
                  params: { type: schema39.properties.added.type },
                  message: 'must be integer,null',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 9007199254740991 || isNaN(data0)) {
                  validate89.errors = [
                    {
                      instancePath: instancePath + '/added',
                      schemaPath: '#/properties/added/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 9007199254740991 },
                      message: 'must be <= 9007199254740991',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate89.errors = [
                      {
                        instancePath: instancePath + '/added',
                        schemaPath: '#/properties/added/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.binary !== undefined) {
              const _errs4 = errors;
              if (typeof data.binary !== 'boolean') {
                validate89.errors = [
                  {
                    instancePath: instancePath + '/binary',
                    schemaPath: '#/properties/binary/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.comparison_note !== undefined) {
                let data2 = data.comparison_note;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate89.errors = [
                    {
                      instancePath: instancePath + '/comparison_note',
                      schemaPath: '#/properties/comparison_note/type',
                      keyword: 'type',
                      params: { type: schema39.properties.comparison_note.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.deleted !== undefined) {
                  let data3 = data.deleted;
                  const _errs8 = errors;
                  if (
                    !(
                      typeof data3 == 'number' &&
                      !(data3 % 1) &&
                      !isNaN(data3) &&
                      isFinite(data3)
                    ) &&
                    data3 !== null
                  ) {
                    validate89.errors = [
                      {
                        instancePath: instancePath + '/deleted',
                        schemaPath: '#/properties/deleted/type',
                        keyword: 'type',
                        params: { type: schema39.properties.deleted.type },
                        message: 'must be integer,null',
                      },
                    ];
                    return false;
                  }
                  if (errors === _errs8) {
                    if (typeof data3 == 'number' && isFinite(data3)) {
                      if (data3 > 9007199254740991 || isNaN(data3)) {
                        validate89.errors = [
                          {
                            instancePath: instancePath + '/deleted',
                            schemaPath: '#/properties/deleted/maximum',
                            keyword: 'maximum',
                            params: { comparison: '<=', limit: 9007199254740991 },
                            message: 'must be <= 9007199254740991',
                          },
                        ];
                        return false;
                      } else {
                        if (data3 < 0 || isNaN(data3)) {
                          validate89.errors = [
                            {
                              instancePath: instancePath + '/deleted',
                              schemaPath: '#/properties/deleted/minimum',
                              keyword: 'minimum',
                              params: { comparison: '>=', limit: 0 },
                              message: 'must be >= 0',
                            },
                          ];
                          return false;
                        }
                      }
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.original_path !== undefined) {
                    let data4 = data.original_path;
                    const _errs10 = errors;
                    if (typeof data4 !== 'string' && data4 !== null) {
                      validate89.errors = [
                        {
                          instancePath: instancePath + '/original_path',
                          schemaPath: '#/properties/original_path/type',
                          keyword: 'type',
                          params: { type: schema39.properties.original_path.type },
                          message: 'must be string,null',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.path !== undefined) {
                      const _errs12 = errors;
                      if (typeof data.path !== 'string') {
                        validate89.errors = [
                          {
                            instancePath: instancePath + '/path',
                            schemaPath: '#/properties/path/type',
                            keyword: 'type',
                            params: { type: 'string' },
                            message: 'must be string',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs12 === errors;
                    } else {
                      var valid0 = true;
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate89.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate89.errors = vErrors;
  return errors === 0;
}
validate89.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate88(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate88.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.warnings === undefined && (missing0 = 'warnings')) ||
        (data.files === undefined && (missing0 = 'files')) ||
        (data.patch === undefined && (missing0 = 'patch')) ||
        (data.offset_bytes === undefined && (missing0 = 'offset_bytes')) ||
        (data.total_bytes === undefined && (missing0 = 'total_bytes')) ||
        (data.truncated === undefined && (missing0 = 'truncated'))
      ) {
        validate88.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'files' ||
            key0 === 'next_offset' ||
            key0 === 'offset_bytes' ||
            key0 === 'patch' ||
            key0 === 'total_bytes' ||
            key0 === 'truncated' ||
            key0 === 'warnings'
          )) {
            validate88.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.files !== undefined) {
            let data0 = data.files;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate89(data0[i0], {
                      instancePath: instancePath + '/files/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate89.errors : vErrors.concat(validate89.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate88.errors = [
                  {
                    instancePath: instancePath + '/files',
                    schemaPath: '#/properties/files/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.next_offset !== undefined) {
              let data2 = data.next_offset;
              const _errs5 = errors;
              if (
                !(typeof data2 == 'number' && !(data2 % 1) && !isNaN(data2) && isFinite(data2)) &&
                data2 !== null
              ) {
                validate88.errors = [
                  {
                    instancePath: instancePath + '/next_offset',
                    schemaPath: '#/properties/next_offset/type',
                    keyword: 'type',
                    params: { type: schema38.properties.next_offset.type },
                    message: 'must be integer,null',
                  },
                ];
                return false;
              }
              if (errors === _errs5) {
                if (typeof data2 == 'number' && isFinite(data2)) {
                  if (data2 > 9007199254740991 || isNaN(data2)) {
                    validate88.errors = [
                      {
                        instancePath: instancePath + '/next_offset',
                        schemaPath: '#/properties/next_offset/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 9007199254740991 },
                        message: 'must be <= 9007199254740991',
                      },
                    ];
                    return false;
                  } else {
                    if (data2 < 0 || isNaN(data2)) {
                      validate88.errors = [
                        {
                          instancePath: instancePath + '/next_offset',
                          schemaPath: '#/properties/next_offset/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs5 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.offset_bytes !== undefined) {
                let data3 = data.offset_bytes;
                const _errs7 = errors;
                if (!(
                  typeof data3 == 'number' &&
                  !(data3 % 1) &&
                  !isNaN(data3) &&
                  isFinite(data3)
                )) {
                  validate88.errors = [
                    {
                      instancePath: instancePath + '/offset_bytes',
                      schemaPath: '#/properties/offset_bytes/type',
                      keyword: 'type',
                      params: { type: 'integer' },
                      message: 'must be integer',
                    },
                  ];
                  return false;
                }
                if (errors === _errs7) {
                  if (typeof data3 == 'number' && isFinite(data3)) {
                    if (data3 > 9007199254740991 || isNaN(data3)) {
                      validate88.errors = [
                        {
                          instancePath: instancePath + '/offset_bytes',
                          schemaPath: '#/properties/offset_bytes/maximum',
                          keyword: 'maximum',
                          params: { comparison: '<=', limit: 9007199254740991 },
                          message: 'must be <= 9007199254740991',
                        },
                      ];
                      return false;
                    } else {
                      if (data3 < 0 || isNaN(data3)) {
                        validate88.errors = [
                          {
                            instancePath: instancePath + '/offset_bytes',
                            schemaPath: '#/properties/offset_bytes/minimum',
                            keyword: 'minimum',
                            params: { comparison: '>=', limit: 0 },
                            message: 'must be >= 0',
                          },
                        ];
                        return false;
                      }
                    }
                  }
                }
                var valid0 = _errs7 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.patch !== undefined) {
                  const _errs9 = errors;
                  if (typeof data.patch !== 'string') {
                    validate88.errors = [
                      {
                        instancePath: instancePath + '/patch',
                        schemaPath: '#/properties/patch/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs9 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.total_bytes !== undefined) {
                    let data5 = data.total_bytes;
                    const _errs11 = errors;
                    if (!(
                      typeof data5 == 'number' &&
                      !(data5 % 1) &&
                      !isNaN(data5) &&
                      isFinite(data5)
                    )) {
                      validate88.errors = [
                        {
                          instancePath: instancePath + '/total_bytes',
                          schemaPath: '#/properties/total_bytes/type',
                          keyword: 'type',
                          params: { type: 'integer' },
                          message: 'must be integer',
                        },
                      ];
                      return false;
                    }
                    if (errors === _errs11) {
                      if (typeof data5 == 'number' && isFinite(data5)) {
                        if (data5 > 9007199254740991 || isNaN(data5)) {
                          validate88.errors = [
                            {
                              instancePath: instancePath + '/total_bytes',
                              schemaPath: '#/properties/total_bytes/maximum',
                              keyword: 'maximum',
                              params: { comparison: '<=', limit: 9007199254740991 },
                              message: 'must be <= 9007199254740991',
                            },
                          ];
                          return false;
                        } else {
                          if (data5 < 0 || isNaN(data5)) {
                            validate88.errors = [
                              {
                                instancePath: instancePath + '/total_bytes',
                                schemaPath: '#/properties/total_bytes/minimum',
                                keyword: 'minimum',
                                params: { comparison: '>=', limit: 0 },
                                message: 'must be >= 0',
                              },
                            ];
                            return false;
                          }
                        }
                      }
                    }
                    var valid0 = _errs11 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.truncated !== undefined) {
                      const _errs13 = errors;
                      if (typeof data.truncated !== 'boolean') {
                        validate88.errors = [
                          {
                            instancePath: instancePath + '/truncated',
                            schemaPath: '#/properties/truncated/type',
                            keyword: 'type',
                            params: { type: 'boolean' },
                            message: 'must be boolean',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs13 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.warnings !== undefined) {
                        let data7 = data.warnings;
                        const _errs15 = errors;
                        if (errors === _errs15) {
                          if (Array.isArray(data7)) {
                            var valid2 = true;
                            const len1 = data7.length;
                            for (let i1 = 0; i1 < len1; i1++) {
                              const _errs17 = errors;
                              if (typeof data7[i1] !== 'string') {
                                validate88.errors = [
                                  {
                                    instancePath: instancePath + '/warnings/' + i1,
                                    schemaPath: '#/properties/warnings/items/type',
                                    keyword: 'type',
                                    params: { type: 'string' },
                                    message: 'must be string',
                                  },
                                ];
                                return false;
                              }
                              var valid2 = _errs17 === errors;
                              if (!valid2) {
                                break;
                              }
                            }
                          } else {
                            validate88.errors = [
                              {
                                instancePath: instancePath + '/warnings',
                                schemaPath: '#/properties/warnings/type',
                                keyword: 'type',
                                params: { type: 'array' },
                                message: 'must be array',
                              },
                            ];
                            return false;
                          }
                        }
                        var valid0 = _errs15 === errors;
                      } else {
                        var valid0 = true;
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate88.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate88.errors = vErrors;
  return errors === 0;
}
validate88.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema40 = {
  additionalProperties: false,
  properties: {
    commits: { items: { $ref: '#/$defs/GitCommitV1' }, type: 'array' },
    truncated: { type: 'boolean' },
  },
  required: ['commits', 'truncated'],
  type: 'object',
};
const schema41 = {
  additionalProperties: false,
  properties: {
    author: { type: 'string' },
    authored_at: { type: 'string' },
    body: { type: 'string' },
    commit: { type: 'string' },
    files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
    parents: { items: { type: 'string' }, type: 'array' },
    subject: { type: 'string' },
  },
  required: ['commit', 'parents', 'author', 'authored_at', 'subject', 'body', 'files'],
  type: 'object',
};
function validate93(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate93.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.commit === undefined && (missing0 = 'commit')) ||
        (data.parents === undefined && (missing0 = 'parents')) ||
        (data.author === undefined && (missing0 = 'author')) ||
        (data.authored_at === undefined && (missing0 = 'authored_at')) ||
        (data.subject === undefined && (missing0 = 'subject')) ||
        (data.body === undefined && (missing0 = 'body')) ||
        (data.files === undefined && (missing0 = 'files'))
      ) {
        validate93.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'author' ||
            key0 === 'authored_at' ||
            key0 === 'body' ||
            key0 === 'commit' ||
            key0 === 'files' ||
            key0 === 'parents' ||
            key0 === 'subject'
          )) {
            validate93.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.author !== undefined) {
            const _errs2 = errors;
            if (typeof data.author !== 'string') {
              validate93.errors = [
                {
                  instancePath: instancePath + '/author',
                  schemaPath: '#/properties/author/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.authored_at !== undefined) {
              const _errs4 = errors;
              if (typeof data.authored_at !== 'string') {
                validate93.errors = [
                  {
                    instancePath: instancePath + '/authored_at',
                    schemaPath: '#/properties/authored_at/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.body !== undefined) {
                const _errs6 = errors;
                if (typeof data.body !== 'string') {
                  validate93.errors = [
                    {
                      instancePath: instancePath + '/body',
                      schemaPath: '#/properties/body/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.commit !== undefined) {
                  const _errs8 = errors;
                  if (typeof data.commit !== 'string') {
                    validate93.errors = [
                      {
                        instancePath: instancePath + '/commit',
                        schemaPath: '#/properties/commit/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.files !== undefined) {
                    let data4 = data.files;
                    const _errs10 = errors;
                    if (errors === _errs10) {
                      if (Array.isArray(data4)) {
                        var valid1 = true;
                        const len0 = data4.length;
                        for (let i0 = 0; i0 < len0; i0++) {
                          const _errs12 = errors;
                          if (
                            !validate89(data4[i0], {
                              instancePath: instancePath + '/files/' + i0,
                              parentData: data4,
                              parentDataProperty: i0,
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate89.errors
                                : vErrors.concat(validate89.errors);
                            errors = vErrors.length;
                          }
                          var valid1 = _errs12 === errors;
                          if (!valid1) {
                            break;
                          }
                        }
                      } else {
                        validate93.errors = [
                          {
                            instancePath: instancePath + '/files',
                            schemaPath: '#/properties/files/type',
                            keyword: 'type',
                            params: { type: 'array' },
                            message: 'must be array',
                          },
                        ];
                        return false;
                      }
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.parents !== undefined) {
                      let data6 = data.parents;
                      const _errs13 = errors;
                      if (errors === _errs13) {
                        if (Array.isArray(data6)) {
                          var valid2 = true;
                          const len1 = data6.length;
                          for (let i1 = 0; i1 < len1; i1++) {
                            const _errs15 = errors;
                            if (typeof data6[i1] !== 'string') {
                              validate93.errors = [
                                {
                                  instancePath: instancePath + '/parents/' + i1,
                                  schemaPath: '#/properties/parents/items/type',
                                  keyword: 'type',
                                  params: { type: 'string' },
                                  message: 'must be string',
                                },
                              ];
                              return false;
                            }
                            var valid2 = _errs15 === errors;
                            if (!valid2) {
                              break;
                            }
                          }
                        } else {
                          validate93.errors = [
                            {
                              instancePath: instancePath + '/parents',
                              schemaPath: '#/properties/parents/type',
                              keyword: 'type',
                              params: { type: 'array' },
                              message: 'must be array',
                            },
                          ];
                          return false;
                        }
                      }
                      var valid0 = _errs13 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.subject !== undefined) {
                        const _errs17 = errors;
                        if (typeof data.subject !== 'string') {
                          validate93.errors = [
                            {
                              instancePath: instancePath + '/subject',
                              schemaPath: '#/properties/subject/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs17 === errors;
                      } else {
                        var valid0 = true;
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate93.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate93.errors = vErrors;
  return errors === 0;
}
validate93.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate92(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate92.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.commits === undefined && (missing0 = 'commits')) ||
        (data.truncated === undefined && (missing0 = 'truncated'))
      ) {
        validate92.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'commits' || key0 === 'truncated')) {
            validate92.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.commits !== undefined) {
            let data0 = data.commits;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (Array.isArray(data0)) {
                var valid1 = true;
                const len0 = data0.length;
                for (let i0 = 0; i0 < len0; i0++) {
                  const _errs4 = errors;
                  if (
                    !validate93(data0[i0], {
                      instancePath: instancePath + '/commits/' + i0,
                      parentData: data0,
                      parentDataProperty: i0,
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate93.errors : vErrors.concat(validate93.errors);
                    errors = vErrors.length;
                  }
                  var valid1 = _errs4 === errors;
                  if (!valid1) {
                    break;
                  }
                }
              } else {
                validate92.errors = [
                  {
                    instancePath: instancePath + '/commits',
                    schemaPath: '#/properties/commits/type',
                    keyword: 'type',
                    params: { type: 'array' },
                    message: 'must be array',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.truncated !== undefined) {
              const _errs5 = errors;
              if (typeof data.truncated !== 'boolean') {
                validate92.errors = [
                  {
                    instancePath: instancePath + '/truncated',
                    schemaPath: '#/properties/truncated/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs5 === errors;
            } else {
              var valid0 = true;
            }
          }
        }
      }
    } else {
      validate92.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate92.errors = vErrors;
  return errors === 0;
}
validate92.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate81(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate81.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.kind === undefined && (missing0 = 'kind')) ||
        (data.result === undefined && (missing0 = 'result'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'kind' || key0 === 'result')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.kind !== undefined) {
            let data0 = data.kind;
            const _errs4 = errors;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('status' !== data0) {
              const err3 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'status' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.result !== undefined) {
              const _errs6 = errors;
              if (
                !validate82(data.result, {
                  instancePath: instancePath + '/result',
                  parentData: data,
                  parentDataProperty: 'result',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate82.errors : vErrors.concat(validate82.errors);
                errors = vErrors.length;
              }
              var valid1 = _errs6 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err4 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err4];
      } else {
        vErrors.push(err4);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs7 = errors;
  if (errors === _errs7) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.kind === undefined && (missing1 = 'kind')) ||
        (data.result === undefined && (missing1 = 'result'))
      ) {
        const err5 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err5];
        } else {
          vErrors.push(err5);
        }
        errors++;
      } else {
        const _errs9 = errors;
        for (const key1 in data) {
          if (!(key1 === 'kind' || key1 === 'result')) {
            const err6 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err6];
            } else {
              vErrors.push(err6);
            }
            errors++;
            break;
          }
        }
        if (_errs9 === errors) {
          if (data.kind !== undefined) {
            let data2 = data.kind;
            const _errs10 = errors;
            if (typeof data2 !== 'string') {
              const err7 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/1/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err7];
              } else {
                vErrors.push(err7);
              }
              errors++;
            }
            if ('diff' !== data2) {
              const err8 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/1/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'diff' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
            var valid2 = _errs10 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.result !== undefined) {
              const _errs12 = errors;
              if (
                !validate88(data.result, {
                  instancePath: instancePath + '/result',
                  parentData: data,
                  parentDataProperty: 'result',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate88.errors : vErrors.concat(validate88.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs12 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs7 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
    const _errs13 = errors;
    if (errors === _errs13) {
      if (data && typeof data == 'object' && !Array.isArray(data)) {
        let missing2;
        if (
          (data.kind === undefined && (missing2 = 'kind')) ||
          (data.result === undefined && (missing2 = 'result'))
        ) {
          const err10 = {
            instancePath,
            schemaPath: '#/oneOf/2/required',
            keyword: 'required',
            params: { missingProperty: missing2 },
            message: "must have required property '" + missing2 + "'",
          };
          if (vErrors === null) {
            vErrors = [err10];
          } else {
            vErrors.push(err10);
          }
          errors++;
        } else {
          const _errs15 = errors;
          for (const key2 in data) {
            if (!(key2 === 'kind' || key2 === 'result')) {
              const err11 = {
                instancePath,
                schemaPath: '#/oneOf/2/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key2 },
                message: 'must NOT have additional properties',
              };
              if (vErrors === null) {
                vErrors = [err11];
              } else {
                vErrors.push(err11);
              }
              errors++;
              break;
            }
          }
          if (_errs15 === errors) {
            if (data.kind !== undefined) {
              let data4 = data.kind;
              const _errs16 = errors;
              if (typeof data4 !== 'string') {
                const err12 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/2/properties/kind/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err12];
                } else {
                  vErrors.push(err12);
                }
                errors++;
              }
              if ('log' !== data4) {
                const err13 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/2/properties/kind/const',
                  keyword: 'const',
                  params: { allowedValue: 'log' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err13];
                } else {
                  vErrors.push(err13);
                }
                errors++;
              }
              var valid3 = _errs16 === errors;
            } else {
              var valid3 = true;
            }
            if (valid3) {
              if (data.result !== undefined) {
                const _errs18 = errors;
                if (
                  !validate92(data.result, {
                    instancePath: instancePath + '/result',
                    parentData: data,
                    parentDataProperty: 'result',
                    rootData,
                    dynamicAnchors,
                  })
                ) {
                  vErrors =
                    vErrors === null ? validate92.errors : vErrors.concat(validate92.errors);
                  errors = vErrors.length;
                }
                var valid3 = _errs18 === errors;
              } else {
                var valid3 = true;
              }
            }
          }
        }
      } else {
        const err14 = {
          instancePath,
          schemaPath: '#/oneOf/2/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        };
        if (vErrors === null) {
          vErrors = [err14];
        } else {
          vErrors.push(err14);
        }
        errors++;
      }
    }
    var _valid0 = _errs13 === errors;
    if (_valid0 && valid0) {
      valid0 = false;
      passing0 = [passing0, 2];
    } else {
      if (_valid0) {
        valid0 = true;
        passing0 = 2;
        if (props0 !== true) {
          props0 = true;
        }
      }
    }
  }
  if (!valid0) {
    const err15 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err15];
    } else {
      vErrors.push(err15);
    }
    errors++;
    validate81.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate81.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate81.evaluated = { dynamicProps: true, dynamicItems: false };
const schema42 = {
  additionalProperties: false,
  properties: { opened: { type: 'boolean' } },
  required: ['opened'],
  type: 'object',
};
function validate98(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate98.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.opened === undefined && (missing0 = 'opened')) {
        validate98.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'opened')) {
            validate98.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.opened !== undefined) {
            if (typeof data.opened !== 'boolean') {
              validate98.errors = [
                {
                  instancePath: instancePath + '/opened',
                  schemaPath: '#/properties/opened/type',
                  keyword: 'type',
                  params: { type: 'boolean' },
                  message: 'must be boolean',
                },
              ];
              return false;
            }
          }
        }
      }
    } else {
      validate98.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate98.errors = vErrors;
  return errors === 0;
}
validate98.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate76(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate76.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.command === undefined && (missing0 = 'command')) ||
        (data.value === undefined && (missing0 = 'value'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/anyOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command' || key0 === 'value')) {
            const err1 = {
              instancePath,
              schemaPath: '#/anyOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.command !== undefined) {
            const _errs4 = errors;
            if ('git.runtime.get' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.runtime.get' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.value !== undefined) {
              const _errs5 = errors;
              if (
                !validate77(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate77.errors : vErrors.concat(validate77.errors);
                errors = vErrors.length;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err3 = {
        instancePath,
        schemaPath: '#/anyOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err3];
      } else {
        vErrors.push(err3);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.command === undefined && (missing1 = 'command')) ||
        (data.value === undefined && (missing1 = 'value'))
      ) {
        const err4 = {
          instancePath,
          schemaPath: '#/anyOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err4];
        } else {
          vErrors.push(err4);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'command' || key1 === 'value')) {
            const err5 = {
              instancePath,
              schemaPath: '#/anyOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err5];
            } else {
              vErrors.push(err5);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.command !== undefined) {
            const _errs9 = errors;
            if ('git.workspace.read' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.workspace.read' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err6];
              } else {
                vErrors.push(err6);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.value !== undefined) {
              const _errs10 = errors;
              if (
                !validate81(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate81.errors : vErrors.concat(validate81.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs10 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err7 = {
        instancePath,
        schemaPath: '#/anyOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err7];
      } else {
        vErrors.push(err7);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs11 = errors;
  if (errors === _errs11) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing2;
      if (
        (data.command === undefined && (missing2 = 'command')) ||
        (data.value === undefined && (missing2 = 'value'))
      ) {
        const err8 = {
          instancePath,
          schemaPath: '#/anyOf/2/required',
          keyword: 'required',
          params: { missingProperty: missing2 },
          message: "must have required property '" + missing2 + "'",
        };
        if (vErrors === null) {
          vErrors = [err8];
        } else {
          vErrors.push(err8);
        }
        errors++;
      } else {
        const _errs13 = errors;
        for (const key2 in data) {
          if (!(key2 === 'command' || key2 === 'value')) {
            const err9 = {
              instancePath,
              schemaPath: '#/anyOf/2/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err9];
            } else {
              vErrors.push(err9);
            }
            errors++;
            break;
          }
        }
        if (_errs13 === errors) {
          if (data.command !== undefined) {
            const _errs14 = errors;
            if ('git.installation.open' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.installation.open' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err10];
              } else {
                vErrors.push(err10);
              }
              errors++;
            }
            var valid3 = _errs14 === errors;
          } else {
            var valid3 = true;
          }
          if (valid3) {
            if (data.value !== undefined) {
              const _errs15 = errors;
              if (
                !validate98(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors = vErrors === null ? validate98.errors : vErrors.concat(validate98.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
          }
        }
      }
    } else {
      const err11 = {
        instancePath,
        schemaPath: '#/anyOf/2/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err11];
      } else {
        vErrors.push(err11);
      }
      errors++;
    }
  }
  var _valid0 = _errs11 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err12 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err12];
    } else {
      vErrors.push(err12);
    }
    errors++;
    validate76.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate76.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate76.evaluated = { dynamicProps: true, dynamicItems: false };
export const validateGitWriteContract = validate100;
const schema43 = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  anyOf: [
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.commit.input' },
        value: { $ref: '#/$defs/GitCommitInput' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.branch.input' },
        value: { $ref: '#/$defs/GitBranchInput' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.commit.result' },
        value: { $ref: '#/$defs/GitCommitResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.branch.result' },
        value: { $ref: '#/$defs/GitBranchResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: { command: { const: 'git.sync.input' }, value: { $ref: '#/$defs/GitSyncInput' } },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: { command: { const: 'git.push.input' }, value: { $ref: '#/$defs/GitPushInput' } },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.sync.result' },
        value: { $ref: '#/$defs/GitRemoteResultV1' },
      },
    },
    {
      type: 'object',
      additionalProperties: false,
      required: ['command', 'value'],
      properties: {
        command: { const: 'git.push.result' },
        value: { $ref: '#/$defs/GitRemoteResultV1' },
      },
    },
  ],
  $defs: {
    EmptyToolInput: { additionalProperties: false, type: 'object' },
    GitRuntimeStatusV1: {
      additionalProperties: false,
      properties: {
        executable: { type: ['string', 'null'] },
        installation_url: { type: 'string' },
        message: { type: 'string' },
        next_step: { type: 'string' },
        state: { $ref: '#/$defs/GitRuntimeStateV1' },
        version: { type: ['string', 'null'] },
      },
      required: ['state', 'message', 'next_step', 'installation_url'],
      type: 'object',
    },
    GitRuntimeStateV1: { enum: ['available', 'missing', 'too_old', 'invalid'], type: 'string' },
    GitWorkspaceReadRequestV1: {
      additionalProperties: false,
      properties: {
        query: { $ref: '#/$defs/GitReadInputV1' },
        session_id: { $ref: '#/$defs/SessionId' },
        workspace_id: { $ref: '#/$defs/WorkspaceId' },
      },
      required: ['workspace_id', 'session_id', 'query'],
      type: 'object',
    },
    GitReadInputV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitStatusInput' },
            kind: { const: 'status', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitDiffInput' },
            kind: { const: 'diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            input: { $ref: '#/$defs/GitLogInput' },
            kind: { const: 'log', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          description: "Desktop review only. The model's git.diff schema remains GitDiffInput.",
          properties: {
            input: { $ref: '#/$defs/GitUntrackedDiffInputV1' },
            kind: { const: 'untracked_diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
        {
          additionalProperties: false,
          description:
            "Desktop review only: the working tree against HEAD or against where HEAD left a\nbranch. The model's git.diff schema remains GitDiffInput.",
          properties: {
            input: { $ref: '#/$defs/GitReviewDiffInputV1' },
            kind: { const: 'review_diff', type: 'string' },
          },
          required: ['kind', 'input'],
          type: 'object',
        },
      ],
    },
    GitStatusInput: {
      additionalProperties: false,
      properties: {
        include_branches: { default: false, type: 'boolean' },
        include_untracked: { default: true, type: 'boolean' },
        limit: { default: 200, maximum: 2000, minimum: 1, type: 'integer' },
        offset: { default: 0, minimum: 0, type: 'integer', maximum: 4294967295 },
      },
      type: 'object',
    },
    GitDiffInput: {
      additionalProperties: false,
      properties: {
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        target: { $ref: '#/$defs/GitDiffTargetV1', default: { kind: 'unstaged' } },
      },
      type: 'object',
    },
    GitDiffTargetV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: { kind: { const: 'unstaged', type: 'string' } },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { kind: { const: 'staged', type: 'string' } },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: { commit: { type: 'string' }, kind: { const: 'commit', type: 'string' } },
          required: ['kind', 'commit'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            from: { type: 'string' },
            kind: { const: 'range', type: 'string' },
            to: { type: 'string' },
          },
          required: ['kind', 'from', 'to'],
          type: 'object',
        },
      ],
    },
    GitLogInput: {
      additionalProperties: false,
      properties: {
        commit: { default: null, type: ['string', 'null'] },
        limit: { default: 20, maximum: 200, minimum: 1, type: 'integer' },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        start: { default: null, type: ['string', 'null'] },
      },
      type: 'object',
    },
    GitUntrackedDiffInputV1: {
      additionalProperties: false,
      properties: {
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        path: { type: 'string' },
      },
      required: ['path'],
      type: 'object',
    },
    GitReviewDiffInputV1: {
      additionalProperties: false,
      description:
        "The review page's comparison. Untracked files are not part of it: status lists them and\n`UntrackedDiff` reads them one by one.",
      properties: {
        base: {
          default: null,
          description:
            'None: the uncommitted changes, staged or not, against HEAD (the empty tree before the\nfirst commit). A branch: the working tree against the commit where HEAD left it.',
          type: ['string', 'null'],
        },
        ignore_whitespace: {
          default: false,
          description:
            'Lines that differ only in whitespace are not changes; files with nothing else drop out.',
          type: 'boolean',
        },
        max_bytes: { default: 16384, maximum: 65536, minimum: 1, type: 'integer' },
        offset_bytes: { default: 0, minimum: 0, type: 'integer', maximum: 9007199254740991 },
        paths: { default: [], items: { type: 'string' }, type: 'array' },
        stats_only: {
          default: false,
          description:
            "Only the files and their line counts, with an empty patch. The review page lists\nthe files this way and reads each file's lines when it is shown.",
          type: 'boolean',
        },
      },
      type: 'object',
    },
    SessionId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^ses_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    WorkspaceId: {
      type: 'string',
      minLength: 30,
      maxLength: 30,
      pattern: '^wsp_[0-7][0-9A-HJKMNP-TV-Z]{25}$',
    },
    GitReadResultV1: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'status', type: 'string' },
            result: { $ref: '#/$defs/GitStatusResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'diff', type: 'string' },
            result: { $ref: '#/$defs/GitDiffResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'log', type: 'string' },
            result: { $ref: '#/$defs/GitLogResultV1' },
          },
          required: ['kind', 'result'],
          type: 'object',
        },
      ],
    },
    GitStatusResultV1: {
      additionalProperties: false,
      properties: {
        ahead: { minimum: 0, type: 'integer', maximum: 4294967295 },
        behind: { minimum: 0, type: 'integer', maximum: 4294967295 },
        branch: { type: ['string', 'null'] },
        branches: { items: { $ref: '#/$defs/GitBranchV1' }, type: 'array' },
        conflicts: { items: { type: 'string' }, type: 'array' },
        detached: { type: 'boolean' },
        entries: { items: { $ref: '#/$defs/GitChangeV1' }, type: 'array' },
        head: { type: ['string', 'null'] },
        next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 4294967295 },
        operation: { type: ['string', 'null'] },
        submodule_contents_checked: {
          description: 'Submodule contents are deliberately not traversed or executed.',
          type: 'boolean',
        },
        total_entries: { minimum: 0, type: 'integer', maximum: 4294967295 },
        truncated: { type: 'boolean' },
        upstream: { type: ['string', 'null'] },
      },
      required: [
        'detached',
        'ahead',
        'behind',
        'conflicts',
        'entries',
        'branches',
        'total_entries',
        'truncated',
        'submodule_contents_checked',
      ],
      type: 'object',
    },
    GitBranchV1: {
      additionalProperties: false,
      properties: {
        commit: { type: 'string' },
        name: { type: 'string' },
        remote: { type: 'boolean' },
      },
      required: ['name', 'commit', 'remote'],
      type: 'object',
    },
    GitChangeV1: {
      additionalProperties: false,
      properties: {
        comparison_note: {
          description:
            'Present only when an attribute selects an external converter suppressed by isolated reads.',
          type: ['string', 'null'],
        },
        conflict: { type: 'boolean' },
        index_status: { type: 'string' },
        original_path: { type: ['string', 'null'] },
        path: { type: 'string' },
        submodule: { type: 'boolean' },
        untracked: { type: 'boolean' },
        worktree_status: { type: 'string' },
      },
      required: ['path', 'index_status', 'worktree_status', 'conflict', 'untracked', 'submodule'],
      type: 'object',
    },
    GitDiffResultV1: {
      additionalProperties: false,
      properties: {
        files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
        next_offset: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        offset_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        patch: { type: 'string' },
        total_bytes: { minimum: 0, type: 'integer', maximum: 9007199254740991 },
        truncated: { type: 'boolean' },
        warnings: { items: { type: 'string' }, type: 'array' },
      },
      required: ['warnings', 'files', 'patch', 'offset_bytes', 'total_bytes', 'truncated'],
      type: 'object',
    },
    GitFileStatV1: {
      additionalProperties: false,
      properties: {
        added: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        binary: { type: 'boolean' },
        comparison_note: { type: ['string', 'null'] },
        deleted: { minimum: 0, type: ['integer', 'null'], maximum: 9007199254740991 },
        original_path: { type: ['string', 'null'] },
        path: { type: 'string' },
      },
      required: ['path', 'binary'],
      type: 'object',
    },
    GitLogResultV1: {
      additionalProperties: false,
      properties: {
        commits: { items: { $ref: '#/$defs/GitCommitV1' }, type: 'array' },
        truncated: { type: 'boolean' },
      },
      required: ['commits', 'truncated'],
      type: 'object',
    },
    GitCommitV1: {
      additionalProperties: false,
      properties: {
        author: { type: 'string' },
        authored_at: { type: 'string' },
        body: { type: 'string' },
        commit: { type: 'string' },
        files: { items: { $ref: '#/$defs/GitFileStatV1' }, type: 'array' },
        parents: { items: { type: 'string' }, type: 'array' },
        subject: { type: 'string' },
      },
      required: ['commit', 'parents', 'author', 'authored_at', 'subject', 'body', 'files'],
      type: 'object',
    },
    GitInstallationResultV1: {
      additionalProperties: false,
      properties: { opened: { type: 'boolean' } },
      required: ['opened'],
      type: 'object',
    },
    GitCommitInput: {
      additionalProperties: false,
      properties: {
        message: {
          description: 'Required non-empty commit message. No amend, editor or hook bypass option.',
          maxLength: 65536,
          minLength: 1,
          type: 'string',
        },
        paths: {
          default: [],
          description:
            'Literal workspace-relative files to stage and commit; excludes unrelated staged files.',
          items: { type: 'string' },
          maxItems: 200,
          type: 'array',
        },
        staged_only: {
          default: false,
          description: 'Commit the existing index only. Mutually exclusive with non-empty paths.',
          type: 'boolean',
        },
      },
      required: ['message'],
      type: 'object',
    },
    GitBranchInput: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'create', type: 'string' },
            name: { maxLength: 1024, minLength: 1, type: 'string' },
            start: { default: null, type: ['string', 'null'] },
            switch: { default: false, type: 'boolean' },
          },
          required: ['kind', 'name'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'switch', type: 'string' },
            name: { maxLength: 1024, minLength: 1, type: 'string' },
          },
          required: ['kind', 'name'],
          type: 'object',
        },
      ],
      type: 'object',
    },
    GitCommitResultV1: {
      additionalProperties: false,
      properties: {
        branch: { type: ['string', 'null'] },
        commit: { $ref: '#/$defs/GitCommitV1' },
        execution_notice: { type: 'string' },
        status: {
          $ref: '#/$defs/GitStatusResultV1',
          description:
            'Bounded post-operation status, including remaining untracked and staged changes.',
        },
      },
      required: ['commit', 'status', 'execution_notice'],
      type: 'object',
    },
    GitBranchResultV1: {
      additionalProperties: false,
      properties: {
        branch: { type: 'string' },
        created: { type: 'boolean' },
        execution_notice: { type: 'string' },
        head: { type: ['string', 'null'] },
        previous_branch: { type: ['string', 'null'] },
        previous_head: { type: ['string', 'null'] },
        status: { $ref: '#/$defs/GitStatusResultV1' },
        switched: { type: 'boolean' },
      },
      required: ['branch', 'created', 'switched', 'status', 'execution_notice'],
      type: 'object',
    },
    GitSyncInput: {
      oneOf: [
        {
          additionalProperties: false,
          properties: {
            kind: { const: 'fetch', type: 'string' },
            remote: { default: null, type: ['string', 'null'] },
          },
          required: ['kind'],
          type: 'object',
        },
        {
          additionalProperties: false,
          properties: {
            branch: { default: null, type: ['string', 'null'] },
            kind: { const: 'pull', type: 'string' },
            remote: { default: null, type: ['string', 'null'] },
          },
          required: ['kind'],
          type: 'object',
        },
      ],
      type: 'object',
    },
    GitPushInput: {
      additionalProperties: false,
      properties: {
        branch: { default: null, type: ['string', 'null'] },
        force_with_lease: {
          default: null,
          description:
            'Full expected remote object ID. An exact lease, never a bare force or tracking-ref lease.',
          type: ['string', 'null'],
        },
        remote: { default: null, type: ['string', 'null'] },
        remote_branch: { default: null, type: ['string', 'null'] },
        set_upstream: { default: false, type: 'boolean' },
      },
      type: 'object',
    },
    GitRemoteResultV1: {
      additionalProperties: false,
      properties: {
        execution_notice: { type: 'string' },
        kind: { type: 'string' },
        local_branch: { type: ['string', 'null'] },
        refs: { items: { $ref: '#/$defs/GitRefUpdateV1' }, type: 'array' },
        remote: { type: 'string' },
        remote_branch: { type: ['string', 'null'] },
        remote_url: {
          description: 'Normalized display and consent URL; has no userinfo, query or fragment.',
          type: 'string',
        },
        status: { $ref: '#/$defs/GitStatusResultV1' },
      },
      required: ['kind', 'remote', 'remote_url', 'refs', 'status', 'execution_notice'],
      type: 'object',
    },
    GitRefUpdateV1: {
      additionalProperties: false,
      properties: {
        after: { type: ['string', 'null'] },
        before: { type: ['string', 'null'] },
        reference: { type: 'string' },
        result: { type: 'string' },
      },
      required: ['reference', 'result'],
      type: 'object',
    },
  },
};
const schema44 = {
  additionalProperties: false,
  properties: {
    message: {
      description: 'Required non-empty commit message. No amend, editor or hook bypass option.',
      maxLength: 65536,
      minLength: 1,
      type: 'string',
    },
    paths: {
      default: [],
      description:
        'Literal workspace-relative files to stage and commit; excludes unrelated staged files.',
      items: { type: 'string' },
      maxItems: 200,
      type: 'array',
    },
    staged_only: {
      default: false,
      description: 'Commit the existing index only. Mutually exclusive with non-empty paths.',
      type: 'boolean',
    },
  },
  required: ['message'],
  type: 'object',
};
function validate101(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate101.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.message === undefined && (missing0 = 'message')) {
        validate101.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'message' || key0 === 'paths' || key0 === 'staged_only')) {
            validate101.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.message !== undefined) {
            let data0 = data.message;
            const _errs2 = errors;
            if (errors === _errs2) {
              if (typeof data0 === 'string') {
                if (func1(data0) > 65536) {
                  validate101.errors = [
                    {
                      instancePath: instancePath + '/message',
                      schemaPath: '#/properties/message/maxLength',
                      keyword: 'maxLength',
                      params: { limit: 65536 },
                      message: 'must NOT have more than 65536 characters',
                    },
                  ];
                  return false;
                } else {
                  if (func1(data0) < 1) {
                    validate101.errors = [
                      {
                        instancePath: instancePath + '/message',
                        schemaPath: '#/properties/message/minLength',
                        keyword: 'minLength',
                        params: { limit: 1 },
                        message: 'must NOT have fewer than 1 characters',
                      },
                    ];
                    return false;
                  }
                }
              } else {
                validate101.errors = [
                  {
                    instancePath: instancePath + '/message',
                    schemaPath: '#/properties/message/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.paths !== undefined) {
              let data1 = data.paths;
              const _errs4 = errors;
              if (errors === _errs4) {
                if (Array.isArray(data1)) {
                  if (data1.length > 200) {
                    validate101.errors = [
                      {
                        instancePath: instancePath + '/paths',
                        schemaPath: '#/properties/paths/maxItems',
                        keyword: 'maxItems',
                        params: { limit: 200 },
                        message: 'must NOT have more than 200 items',
                      },
                    ];
                    return false;
                  } else {
                    var valid1 = true;
                    const len0 = data1.length;
                    for (let i0 = 0; i0 < len0; i0++) {
                      const _errs6 = errors;
                      if (typeof data1[i0] !== 'string') {
                        validate101.errors = [
                          {
                            instancePath: instancePath + '/paths/' + i0,
                            schemaPath: '#/properties/paths/items/type',
                            keyword: 'type',
                            params: { type: 'string' },
                            message: 'must be string',
                          },
                        ];
                        return false;
                      }
                      var valid1 = _errs6 === errors;
                      if (!valid1) {
                        break;
                      }
                    }
                  }
                } else {
                  validate101.errors = [
                    {
                      instancePath: instancePath + '/paths',
                      schemaPath: '#/properties/paths/type',
                      keyword: 'type',
                      params: { type: 'array' },
                      message: 'must be array',
                    },
                  ];
                  return false;
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.staged_only !== undefined) {
                const _errs8 = errors;
                if (typeof data.staged_only !== 'boolean') {
                  validate101.errors = [
                    {
                      instancePath: instancePath + '/staged_only',
                      schemaPath: '#/properties/staged_only/type',
                      keyword: 'type',
                      params: { type: 'boolean' },
                      message: 'must be boolean',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate101.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate101.errors = vErrors;
  return errors === 0;
}
validate101.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema45 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: {
        kind: { const: 'create', type: 'string' },
        name: { maxLength: 1024, minLength: 1, type: 'string' },
        start: { default: null, type: ['string', 'null'] },
        switch: { default: false, type: 'boolean' },
      },
      required: ['kind', 'name'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        kind: { const: 'switch', type: 'string' },
        name: { maxLength: 1024, minLength: 1, type: 'string' },
      },
      required: ['kind', 'name'],
      type: 'object',
    },
  ],
  type: 'object',
};
function validate103(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate103.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (!(data && typeof data == 'object' && !Array.isArray(data))) {
    validate103.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      },
    ];
    return false;
  }
  const _errs1 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs2 = errors;
  if (errors === _errs2) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.kind === undefined && (missing0 = 'kind')) ||
        (data.name === undefined && (missing0 = 'name'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs4 = errors;
        for (const key0 in data) {
          if (!(key0 === 'kind' || key0 === 'name' || key0 === 'start' || key0 === 'switch')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs4 === errors) {
          if (data.kind !== undefined) {
            let data0 = data.kind;
            const _errs5 = errors;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('create' !== data0) {
              const err3 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'create' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
            var valid1 = _errs5 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.name !== undefined) {
              let data1 = data.name;
              const _errs7 = errors;
              if (errors === _errs7) {
                if (typeof data1 === 'string') {
                  if (func1(data1) > 1024) {
                    const err4 = {
                      instancePath: instancePath + '/name',
                      schemaPath: '#/oneOf/0/properties/name/maxLength',
                      keyword: 'maxLength',
                      params: { limit: 1024 },
                      message: 'must NOT have more than 1024 characters',
                    };
                    if (vErrors === null) {
                      vErrors = [err4];
                    } else {
                      vErrors.push(err4);
                    }
                    errors++;
                  } else {
                    if (func1(data1) < 1) {
                      const err5 = {
                        instancePath: instancePath + '/name',
                        schemaPath: '#/oneOf/0/properties/name/minLength',
                        keyword: 'minLength',
                        params: { limit: 1 },
                        message: 'must NOT have fewer than 1 characters',
                      };
                      if (vErrors === null) {
                        vErrors = [err5];
                      } else {
                        vErrors.push(err5);
                      }
                      errors++;
                    }
                  }
                } else {
                  const err6 = {
                    instancePath: instancePath + '/name',
                    schemaPath: '#/oneOf/0/properties/name/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err6];
                  } else {
                    vErrors.push(err6);
                  }
                  errors++;
                }
              }
              var valid1 = _errs7 === errors;
            } else {
              var valid1 = true;
            }
            if (valid1) {
              if (data.start !== undefined) {
                let data2 = data.start;
                const _errs9 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  const err7 = {
                    instancePath: instancePath + '/start',
                    schemaPath: '#/oneOf/0/properties/start/type',
                    keyword: 'type',
                    params: { type: schema45.oneOf[0].properties.start.type },
                    message: 'must be string,null',
                  };
                  if (vErrors === null) {
                    vErrors = [err7];
                  } else {
                    vErrors.push(err7);
                  }
                  errors++;
                }
                var valid1 = _errs9 === errors;
              } else {
                var valid1 = true;
              }
              if (valid1) {
                if (data.switch !== undefined) {
                  const _errs11 = errors;
                  if (typeof data.switch !== 'boolean') {
                    const err8 = {
                      instancePath: instancePath + '/switch',
                      schemaPath: '#/oneOf/0/properties/switch/type',
                      keyword: 'type',
                      params: { type: 'boolean' },
                      message: 'must be boolean',
                    };
                    if (vErrors === null) {
                      vErrors = [err8];
                    } else {
                      vErrors.push(err8);
                    }
                    errors++;
                  }
                  var valid1 = _errs11 === errors;
                } else {
                  var valid1 = true;
                }
              }
            }
          }
        }
      }
    } else {
      const err9 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err9];
      } else {
        vErrors.push(err9);
      }
      errors++;
    }
  }
  var _valid0 = _errs2 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs13 = errors;
  if (errors === _errs13) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.kind === undefined && (missing1 = 'kind')) ||
        (data.name === undefined && (missing1 = 'name'))
      ) {
        const err10 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err10];
        } else {
          vErrors.push(err10);
        }
        errors++;
      } else {
        const _errs15 = errors;
        for (const key1 in data) {
          if (!(key1 === 'kind' || key1 === 'name')) {
            const err11 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err11];
            } else {
              vErrors.push(err11);
            }
            errors++;
            break;
          }
        }
        if (_errs15 === errors) {
          if (data.kind !== undefined) {
            let data4 = data.kind;
            const _errs16 = errors;
            if (typeof data4 !== 'string') {
              const err12 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/1/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err12];
              } else {
                vErrors.push(err12);
              }
              errors++;
            }
            if ('switch' !== data4) {
              const err13 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/1/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'switch' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err13];
              } else {
                vErrors.push(err13);
              }
              errors++;
            }
            var valid2 = _errs16 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.name !== undefined) {
              let data5 = data.name;
              const _errs18 = errors;
              if (errors === _errs18) {
                if (typeof data5 === 'string') {
                  if (func1(data5) > 1024) {
                    const err14 = {
                      instancePath: instancePath + '/name',
                      schemaPath: '#/oneOf/1/properties/name/maxLength',
                      keyword: 'maxLength',
                      params: { limit: 1024 },
                      message: 'must NOT have more than 1024 characters',
                    };
                    if (vErrors === null) {
                      vErrors = [err14];
                    } else {
                      vErrors.push(err14);
                    }
                    errors++;
                  } else {
                    if (func1(data5) < 1) {
                      const err15 = {
                        instancePath: instancePath + '/name',
                        schemaPath: '#/oneOf/1/properties/name/minLength',
                        keyword: 'minLength',
                        params: { limit: 1 },
                        message: 'must NOT have fewer than 1 characters',
                      };
                      if (vErrors === null) {
                        vErrors = [err15];
                      } else {
                        vErrors.push(err15);
                      }
                      errors++;
                    }
                  }
                } else {
                  const err16 = {
                    instancePath: instancePath + '/name',
                    schemaPath: '#/oneOf/1/properties/name/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  };
                  if (vErrors === null) {
                    vErrors = [err16];
                  } else {
                    vErrors.push(err16);
                  }
                  errors++;
                }
              }
              var valid2 = _errs18 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err17 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err17];
      } else {
        vErrors.push(err17);
      }
      errors++;
    }
  }
  var _valid0 = _errs13 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
  }
  if (!valid0) {
    const err18 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err18];
    } else {
      vErrors.push(err18);
    }
    errors++;
    validate103.errors = vErrors;
    return false;
  } else {
    errors = _errs1;
    if (vErrors !== null) {
      if (_errs1) {
        vErrors.length = _errs1;
      } else {
        vErrors = null;
      }
    }
  }
  validate103.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate103.evaluated = { dynamicProps: true, dynamicItems: false };
const schema46 = {
  additionalProperties: false,
  properties: {
    branch: { type: ['string', 'null'] },
    commit: { $ref: '#/$defs/GitCommitV1' },
    execution_notice: { type: 'string' },
    status: {
      $ref: '#/$defs/GitStatusResultV1',
      description:
        'Bounded post-operation status, including remaining untracked and staged changes.',
    },
  },
  required: ['commit', 'status', 'execution_notice'],
  type: 'object',
};
function validate107(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate107.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.path === undefined && (missing0 = 'path')) ||
        (data.binary === undefined && (missing0 = 'binary'))
      ) {
        validate107.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'added' ||
            key0 === 'binary' ||
            key0 === 'comparison_note' ||
            key0 === 'deleted' ||
            key0 === 'original_path' ||
            key0 === 'path'
          )) {
            validate107.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.added !== undefined) {
            let data0 = data.added;
            const _errs2 = errors;
            if (
              !(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0)) &&
              data0 !== null
            ) {
              validate107.errors = [
                {
                  instancePath: instancePath + '/added',
                  schemaPath: '#/properties/added/type',
                  keyword: 'type',
                  params: { type: schema39.properties.added.type },
                  message: 'must be integer,null',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 9007199254740991 || isNaN(data0)) {
                  validate107.errors = [
                    {
                      instancePath: instancePath + '/added',
                      schemaPath: '#/properties/added/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 9007199254740991 },
                      message: 'must be <= 9007199254740991',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate107.errors = [
                      {
                        instancePath: instancePath + '/added',
                        schemaPath: '#/properties/added/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.binary !== undefined) {
              const _errs4 = errors;
              if (typeof data.binary !== 'boolean') {
                validate107.errors = [
                  {
                    instancePath: instancePath + '/binary',
                    schemaPath: '#/properties/binary/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.comparison_note !== undefined) {
                let data2 = data.comparison_note;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate107.errors = [
                    {
                      instancePath: instancePath + '/comparison_note',
                      schemaPath: '#/properties/comparison_note/type',
                      keyword: 'type',
                      params: { type: schema39.properties.comparison_note.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.deleted !== undefined) {
                  let data3 = data.deleted;
                  const _errs8 = errors;
                  if (
                    !(
                      typeof data3 == 'number' &&
                      !(data3 % 1) &&
                      !isNaN(data3) &&
                      isFinite(data3)
                    ) &&
                    data3 !== null
                  ) {
                    validate107.errors = [
                      {
                        instancePath: instancePath + '/deleted',
                        schemaPath: '#/properties/deleted/type',
                        keyword: 'type',
                        params: { type: schema39.properties.deleted.type },
                        message: 'must be integer,null',
                      },
                    ];
                    return false;
                  }
                  if (errors === _errs8) {
                    if (typeof data3 == 'number' && isFinite(data3)) {
                      if (data3 > 9007199254740991 || isNaN(data3)) {
                        validate107.errors = [
                          {
                            instancePath: instancePath + '/deleted',
                            schemaPath: '#/properties/deleted/maximum',
                            keyword: 'maximum',
                            params: { comparison: '<=', limit: 9007199254740991 },
                            message: 'must be <= 9007199254740991',
                          },
                        ];
                        return false;
                      } else {
                        if (data3 < 0 || isNaN(data3)) {
                          validate107.errors = [
                            {
                              instancePath: instancePath + '/deleted',
                              schemaPath: '#/properties/deleted/minimum',
                              keyword: 'minimum',
                              params: { comparison: '>=', limit: 0 },
                              message: 'must be >= 0',
                            },
                          ];
                          return false;
                        }
                      }
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.original_path !== undefined) {
                    let data4 = data.original_path;
                    const _errs10 = errors;
                    if (typeof data4 !== 'string' && data4 !== null) {
                      validate107.errors = [
                        {
                          instancePath: instancePath + '/original_path',
                          schemaPath: '#/properties/original_path/type',
                          keyword: 'type',
                          params: { type: schema39.properties.original_path.type },
                          message: 'must be string,null',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.path !== undefined) {
                      const _errs12 = errors;
                      if (typeof data.path !== 'string') {
                        validate107.errors = [
                          {
                            instancePath: instancePath + '/path',
                            schemaPath: '#/properties/path/type',
                            keyword: 'type',
                            params: { type: 'string' },
                            message: 'must be string',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs12 === errors;
                    } else {
                      var valid0 = true;
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate107.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate107.errors = vErrors;
  return errors === 0;
}
validate107.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate106(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate106.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.commit === undefined && (missing0 = 'commit')) ||
        (data.parents === undefined && (missing0 = 'parents')) ||
        (data.author === undefined && (missing0 = 'author')) ||
        (data.authored_at === undefined && (missing0 = 'authored_at')) ||
        (data.subject === undefined && (missing0 = 'subject')) ||
        (data.body === undefined && (missing0 = 'body')) ||
        (data.files === undefined && (missing0 = 'files'))
      ) {
        validate106.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'author' ||
            key0 === 'authored_at' ||
            key0 === 'body' ||
            key0 === 'commit' ||
            key0 === 'files' ||
            key0 === 'parents' ||
            key0 === 'subject'
          )) {
            validate106.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.author !== undefined) {
            const _errs2 = errors;
            if (typeof data.author !== 'string') {
              validate106.errors = [
                {
                  instancePath: instancePath + '/author',
                  schemaPath: '#/properties/author/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.authored_at !== undefined) {
              const _errs4 = errors;
              if (typeof data.authored_at !== 'string') {
                validate106.errors = [
                  {
                    instancePath: instancePath + '/authored_at',
                    schemaPath: '#/properties/authored_at/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.body !== undefined) {
                const _errs6 = errors;
                if (typeof data.body !== 'string') {
                  validate106.errors = [
                    {
                      instancePath: instancePath + '/body',
                      schemaPath: '#/properties/body/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.commit !== undefined) {
                  const _errs8 = errors;
                  if (typeof data.commit !== 'string') {
                    validate106.errors = [
                      {
                        instancePath: instancePath + '/commit',
                        schemaPath: '#/properties/commit/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.files !== undefined) {
                    let data4 = data.files;
                    const _errs10 = errors;
                    if (errors === _errs10) {
                      if (Array.isArray(data4)) {
                        var valid1 = true;
                        const len0 = data4.length;
                        for (let i0 = 0; i0 < len0; i0++) {
                          const _errs12 = errors;
                          if (
                            !validate107(data4[i0], {
                              instancePath: instancePath + '/files/' + i0,
                              parentData: data4,
                              parentDataProperty: i0,
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate107.errors
                                : vErrors.concat(validate107.errors);
                            errors = vErrors.length;
                          }
                          var valid1 = _errs12 === errors;
                          if (!valid1) {
                            break;
                          }
                        }
                      } else {
                        validate106.errors = [
                          {
                            instancePath: instancePath + '/files',
                            schemaPath: '#/properties/files/type',
                            keyword: 'type',
                            params: { type: 'array' },
                            message: 'must be array',
                          },
                        ];
                        return false;
                      }
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.parents !== undefined) {
                      let data6 = data.parents;
                      const _errs13 = errors;
                      if (errors === _errs13) {
                        if (Array.isArray(data6)) {
                          var valid2 = true;
                          const len1 = data6.length;
                          for (let i1 = 0; i1 < len1; i1++) {
                            const _errs15 = errors;
                            if (typeof data6[i1] !== 'string') {
                              validate106.errors = [
                                {
                                  instancePath: instancePath + '/parents/' + i1,
                                  schemaPath: '#/properties/parents/items/type',
                                  keyword: 'type',
                                  params: { type: 'string' },
                                  message: 'must be string',
                                },
                              ];
                              return false;
                            }
                            var valid2 = _errs15 === errors;
                            if (!valid2) {
                              break;
                            }
                          }
                        } else {
                          validate106.errors = [
                            {
                              instancePath: instancePath + '/parents',
                              schemaPath: '#/properties/parents/type',
                              keyword: 'type',
                              params: { type: 'array' },
                              message: 'must be array',
                            },
                          ];
                          return false;
                        }
                      }
                      var valid0 = _errs13 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.subject !== undefined) {
                        const _errs17 = errors;
                        if (typeof data.subject !== 'string') {
                          validate106.errors = [
                            {
                              instancePath: instancePath + '/subject',
                              schemaPath: '#/properties/subject/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs17 === errors;
                      } else {
                        var valid0 = true;
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate106.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate106.errors = vErrors;
  return errors === 0;
}
validate106.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate111(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate111.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.name === undefined && (missing0 = 'name')) ||
        (data.commit === undefined && (missing0 = 'commit')) ||
        (data.remote === undefined && (missing0 = 'remote'))
      ) {
        validate111.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(key0 === 'commit' || key0 === 'name' || key0 === 'remote')) {
            validate111.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.commit !== undefined) {
            const _errs2 = errors;
            if (typeof data.commit !== 'string') {
              validate111.errors = [
                {
                  instancePath: instancePath + '/commit',
                  schemaPath: '#/properties/commit/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.name !== undefined) {
              const _errs4 = errors;
              if (typeof data.name !== 'string') {
                validate111.errors = [
                  {
                    instancePath: instancePath + '/name',
                    schemaPath: '#/properties/name/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.remote !== undefined) {
                const _errs6 = errors;
                if (typeof data.remote !== 'boolean') {
                  validate111.errors = [
                    {
                      instancePath: instancePath + '/remote',
                      schemaPath: '#/properties/remote/type',
                      keyword: 'type',
                      params: { type: 'boolean' },
                      message: 'must be boolean',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
            }
          }
        }
      }
    } else {
      validate111.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate111.errors = vErrors;
  return errors === 0;
}
validate111.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate113(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate113.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.path === undefined && (missing0 = 'path')) ||
        (data.index_status === undefined && (missing0 = 'index_status')) ||
        (data.worktree_status === undefined && (missing0 = 'worktree_status')) ||
        (data.conflict === undefined && (missing0 = 'conflict')) ||
        (data.untracked === undefined && (missing0 = 'untracked')) ||
        (data.submodule === undefined && (missing0 = 'submodule'))
      ) {
        validate113.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'comparison_note' ||
            key0 === 'conflict' ||
            key0 === 'index_status' ||
            key0 === 'original_path' ||
            key0 === 'path' ||
            key0 === 'submodule' ||
            key0 === 'untracked' ||
            key0 === 'worktree_status'
          )) {
            validate113.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.comparison_note !== undefined) {
            let data0 = data.comparison_note;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate113.errors = [
                {
                  instancePath: instancePath + '/comparison_note',
                  schemaPath: '#/properties/comparison_note/type',
                  keyword: 'type',
                  params: { type: schema37.properties.comparison_note.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.conflict !== undefined) {
              const _errs4 = errors;
              if (typeof data.conflict !== 'boolean') {
                validate113.errors = [
                  {
                    instancePath: instancePath + '/conflict',
                    schemaPath: '#/properties/conflict/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.index_status !== undefined) {
                const _errs6 = errors;
                if (typeof data.index_status !== 'string') {
                  validate113.errors = [
                    {
                      instancePath: instancePath + '/index_status',
                      schemaPath: '#/properties/index_status/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.original_path !== undefined) {
                  let data3 = data.original_path;
                  const _errs8 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate113.errors = [
                      {
                        instancePath: instancePath + '/original_path',
                        schemaPath: '#/properties/original_path/type',
                        keyword: 'type',
                        params: { type: schema37.properties.original_path.type },
                        message: 'must be string,null',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.path !== undefined) {
                    const _errs10 = errors;
                    if (typeof data.path !== 'string') {
                      validate113.errors = [
                        {
                          instancePath: instancePath + '/path',
                          schemaPath: '#/properties/path/type',
                          keyword: 'type',
                          params: { type: 'string' },
                          message: 'must be string',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.submodule !== undefined) {
                      const _errs12 = errors;
                      if (typeof data.submodule !== 'boolean') {
                        validate113.errors = [
                          {
                            instancePath: instancePath + '/submodule',
                            schemaPath: '#/properties/submodule/type',
                            keyword: 'type',
                            params: { type: 'boolean' },
                            message: 'must be boolean',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs12 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.untracked !== undefined) {
                        const _errs14 = errors;
                        if (typeof data.untracked !== 'boolean') {
                          validate113.errors = [
                            {
                              instancePath: instancePath + '/untracked',
                              schemaPath: '#/properties/untracked/type',
                              keyword: 'type',
                              params: { type: 'boolean' },
                              message: 'must be boolean',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs14 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.worktree_status !== undefined) {
                          const _errs16 = errors;
                          if (typeof data.worktree_status !== 'string') {
                            validate113.errors = [
                              {
                                instancePath: instancePath + '/worktree_status',
                                schemaPath: '#/properties/worktree_status/type',
                                keyword: 'type',
                                params: { type: 'string' },
                                message: 'must be string',
                              },
                            ];
                            return false;
                          }
                          var valid0 = _errs16 === errors;
                        } else {
                          var valid0 = true;
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate113.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate113.errors = vErrors;
  return errors === 0;
}
validate113.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate110(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate110.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.detached === undefined && (missing0 = 'detached')) ||
        (data.ahead === undefined && (missing0 = 'ahead')) ||
        (data.behind === undefined && (missing0 = 'behind')) ||
        (data.conflicts === undefined && (missing0 = 'conflicts')) ||
        (data.entries === undefined && (missing0 = 'entries')) ||
        (data.branches === undefined && (missing0 = 'branches')) ||
        (data.total_entries === undefined && (missing0 = 'total_entries')) ||
        (data.truncated === undefined && (missing0 = 'truncated')) ||
        (data.submodule_contents_checked === undefined && (missing0 = 'submodule_contents_checked'))
      ) {
        validate110.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!func5.call(schema35.properties, key0)) {
            validate110.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.ahead !== undefined) {
            let data0 = data.ahead;
            const _errs2 = errors;
            if (!(typeof data0 == 'number' && !(data0 % 1) && !isNaN(data0) && isFinite(data0))) {
              validate110.errors = [
                {
                  instancePath: instancePath + '/ahead',
                  schemaPath: '#/properties/ahead/type',
                  keyword: 'type',
                  params: { type: 'integer' },
                  message: 'must be integer',
                },
              ];
              return false;
            }
            if (errors === _errs2) {
              if (typeof data0 == 'number' && isFinite(data0)) {
                if (data0 > 4294967295 || isNaN(data0)) {
                  validate110.errors = [
                    {
                      instancePath: instancePath + '/ahead',
                      schemaPath: '#/properties/ahead/maximum',
                      keyword: 'maximum',
                      params: { comparison: '<=', limit: 4294967295 },
                      message: 'must be <= 4294967295',
                    },
                  ];
                  return false;
                } else {
                  if (data0 < 0 || isNaN(data0)) {
                    validate110.errors = [
                      {
                        instancePath: instancePath + '/ahead',
                        schemaPath: '#/properties/ahead/minimum',
                        keyword: 'minimum',
                        params: { comparison: '>=', limit: 0 },
                        message: 'must be >= 0',
                      },
                    ];
                    return false;
                  }
                }
              }
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.behind !== undefined) {
              let data1 = data.behind;
              const _errs4 = errors;
              if (!(typeof data1 == 'number' && !(data1 % 1) && !isNaN(data1) && isFinite(data1))) {
                validate110.errors = [
                  {
                    instancePath: instancePath + '/behind',
                    schemaPath: '#/properties/behind/type',
                    keyword: 'type',
                    params: { type: 'integer' },
                    message: 'must be integer',
                  },
                ];
                return false;
              }
              if (errors === _errs4) {
                if (typeof data1 == 'number' && isFinite(data1)) {
                  if (data1 > 4294967295 || isNaN(data1)) {
                    validate110.errors = [
                      {
                        instancePath: instancePath + '/behind',
                        schemaPath: '#/properties/behind/maximum',
                        keyword: 'maximum',
                        params: { comparison: '<=', limit: 4294967295 },
                        message: 'must be <= 4294967295',
                      },
                    ];
                    return false;
                  } else {
                    if (data1 < 0 || isNaN(data1)) {
                      validate110.errors = [
                        {
                          instancePath: instancePath + '/behind',
                          schemaPath: '#/properties/behind/minimum',
                          keyword: 'minimum',
                          params: { comparison: '>=', limit: 0 },
                          message: 'must be >= 0',
                        },
                      ];
                      return false;
                    }
                  }
                }
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.branch !== undefined) {
                let data2 = data.branch;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate110.errors = [
                    {
                      instancePath: instancePath + '/branch',
                      schemaPath: '#/properties/branch/type',
                      keyword: 'type',
                      params: { type: schema35.properties.branch.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.branches !== undefined) {
                  let data3 = data.branches;
                  const _errs8 = errors;
                  if (errors === _errs8) {
                    if (Array.isArray(data3)) {
                      var valid1 = true;
                      const len0 = data3.length;
                      for (let i0 = 0; i0 < len0; i0++) {
                        const _errs10 = errors;
                        if (
                          !validate111(data3[i0], {
                            instancePath: instancePath + '/branches/' + i0,
                            parentData: data3,
                            parentDataProperty: i0,
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate111.errors
                              : vErrors.concat(validate111.errors);
                          errors = vErrors.length;
                        }
                        var valid1 = _errs10 === errors;
                        if (!valid1) {
                          break;
                        }
                      }
                    } else {
                      validate110.errors = [
                        {
                          instancePath: instancePath + '/branches',
                          schemaPath: '#/properties/branches/type',
                          keyword: 'type',
                          params: { type: 'array' },
                          message: 'must be array',
                        },
                      ];
                      return false;
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.conflicts !== undefined) {
                    let data5 = data.conflicts;
                    const _errs11 = errors;
                    if (errors === _errs11) {
                      if (Array.isArray(data5)) {
                        var valid2 = true;
                        const len1 = data5.length;
                        for (let i1 = 0; i1 < len1; i1++) {
                          const _errs13 = errors;
                          if (typeof data5[i1] !== 'string') {
                            validate110.errors = [
                              {
                                instancePath: instancePath + '/conflicts/' + i1,
                                schemaPath: '#/properties/conflicts/items/type',
                                keyword: 'type',
                                params: { type: 'string' },
                                message: 'must be string',
                              },
                            ];
                            return false;
                          }
                          var valid2 = _errs13 === errors;
                          if (!valid2) {
                            break;
                          }
                        }
                      } else {
                        validate110.errors = [
                          {
                            instancePath: instancePath + '/conflicts',
                            schemaPath: '#/properties/conflicts/type',
                            keyword: 'type',
                            params: { type: 'array' },
                            message: 'must be array',
                          },
                        ];
                        return false;
                      }
                    }
                    var valid0 = _errs11 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.detached !== undefined) {
                      const _errs15 = errors;
                      if (typeof data.detached !== 'boolean') {
                        validate110.errors = [
                          {
                            instancePath: instancePath + '/detached',
                            schemaPath: '#/properties/detached/type',
                            keyword: 'type',
                            params: { type: 'boolean' },
                            message: 'must be boolean',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs15 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.entries !== undefined) {
                        let data8 = data.entries;
                        const _errs17 = errors;
                        if (errors === _errs17) {
                          if (Array.isArray(data8)) {
                            var valid3 = true;
                            const len2 = data8.length;
                            for (let i2 = 0; i2 < len2; i2++) {
                              const _errs19 = errors;
                              if (
                                !validate113(data8[i2], {
                                  instancePath: instancePath + '/entries/' + i2,
                                  parentData: data8,
                                  parentDataProperty: i2,
                                  rootData,
                                  dynamicAnchors,
                                })
                              ) {
                                vErrors =
                                  vErrors === null
                                    ? validate113.errors
                                    : vErrors.concat(validate113.errors);
                                errors = vErrors.length;
                              }
                              var valid3 = _errs19 === errors;
                              if (!valid3) {
                                break;
                              }
                            }
                          } else {
                            validate110.errors = [
                              {
                                instancePath: instancePath + '/entries',
                                schemaPath: '#/properties/entries/type',
                                keyword: 'type',
                                params: { type: 'array' },
                                message: 'must be array',
                              },
                            ];
                            return false;
                          }
                        }
                        var valid0 = _errs17 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.head !== undefined) {
                          let data10 = data.head;
                          const _errs20 = errors;
                          if (typeof data10 !== 'string' && data10 !== null) {
                            validate110.errors = [
                              {
                                instancePath: instancePath + '/head',
                                schemaPath: '#/properties/head/type',
                                keyword: 'type',
                                params: { type: schema35.properties.head.type },
                                message: 'must be string,null',
                              },
                            ];
                            return false;
                          }
                          var valid0 = _errs20 === errors;
                        } else {
                          var valid0 = true;
                        }
                        if (valid0) {
                          if (data.next_offset !== undefined) {
                            let data11 = data.next_offset;
                            const _errs22 = errors;
                            if (
                              !(
                                typeof data11 == 'number' &&
                                !(data11 % 1) &&
                                !isNaN(data11) &&
                                isFinite(data11)
                              ) &&
                              data11 !== null
                            ) {
                              validate110.errors = [
                                {
                                  instancePath: instancePath + '/next_offset',
                                  schemaPath: '#/properties/next_offset/type',
                                  keyword: 'type',
                                  params: { type: schema35.properties.next_offset.type },
                                  message: 'must be integer,null',
                                },
                              ];
                              return false;
                            }
                            if (errors === _errs22) {
                              if (typeof data11 == 'number' && isFinite(data11)) {
                                if (data11 > 4294967295 || isNaN(data11)) {
                                  validate110.errors = [
                                    {
                                      instancePath: instancePath + '/next_offset',
                                      schemaPath: '#/properties/next_offset/maximum',
                                      keyword: 'maximum',
                                      params: { comparison: '<=', limit: 4294967295 },
                                      message: 'must be <= 4294967295',
                                    },
                                  ];
                                  return false;
                                } else {
                                  if (data11 < 0 || isNaN(data11)) {
                                    validate110.errors = [
                                      {
                                        instancePath: instancePath + '/next_offset',
                                        schemaPath: '#/properties/next_offset/minimum',
                                        keyword: 'minimum',
                                        params: { comparison: '>=', limit: 0 },
                                        message: 'must be >= 0',
                                      },
                                    ];
                                    return false;
                                  }
                                }
                              }
                            }
                            var valid0 = _errs22 === errors;
                          } else {
                            var valid0 = true;
                          }
                          if (valid0) {
                            if (data.operation !== undefined) {
                              let data12 = data.operation;
                              const _errs24 = errors;
                              if (typeof data12 !== 'string' && data12 !== null) {
                                validate110.errors = [
                                  {
                                    instancePath: instancePath + '/operation',
                                    schemaPath: '#/properties/operation/type',
                                    keyword: 'type',
                                    params: { type: schema35.properties.operation.type },
                                    message: 'must be string,null',
                                  },
                                ];
                                return false;
                              }
                              var valid0 = _errs24 === errors;
                            } else {
                              var valid0 = true;
                            }
                            if (valid0) {
                              if (data.submodule_contents_checked !== undefined) {
                                const _errs26 = errors;
                                if (typeof data.submodule_contents_checked !== 'boolean') {
                                  validate110.errors = [
                                    {
                                      instancePath: instancePath + '/submodule_contents_checked',
                                      schemaPath: '#/properties/submodule_contents_checked/type',
                                      keyword: 'type',
                                      params: { type: 'boolean' },
                                      message: 'must be boolean',
                                    },
                                  ];
                                  return false;
                                }
                                var valid0 = _errs26 === errors;
                              } else {
                                var valid0 = true;
                              }
                              if (valid0) {
                                if (data.total_entries !== undefined) {
                                  let data14 = data.total_entries;
                                  const _errs28 = errors;
                                  if (!(
                                    typeof data14 == 'number' &&
                                    !(data14 % 1) &&
                                    !isNaN(data14) &&
                                    isFinite(data14)
                                  )) {
                                    validate110.errors = [
                                      {
                                        instancePath: instancePath + '/total_entries',
                                        schemaPath: '#/properties/total_entries/type',
                                        keyword: 'type',
                                        params: { type: 'integer' },
                                        message: 'must be integer',
                                      },
                                    ];
                                    return false;
                                  }
                                  if (errors === _errs28) {
                                    if (typeof data14 == 'number' && isFinite(data14)) {
                                      if (data14 > 4294967295 || isNaN(data14)) {
                                        validate110.errors = [
                                          {
                                            instancePath: instancePath + '/total_entries',
                                            schemaPath: '#/properties/total_entries/maximum',
                                            keyword: 'maximum',
                                            params: { comparison: '<=', limit: 4294967295 },
                                            message: 'must be <= 4294967295',
                                          },
                                        ];
                                        return false;
                                      } else {
                                        if (data14 < 0 || isNaN(data14)) {
                                          validate110.errors = [
                                            {
                                              instancePath: instancePath + '/total_entries',
                                              schemaPath: '#/properties/total_entries/minimum',
                                              keyword: 'minimum',
                                              params: { comparison: '>=', limit: 0 },
                                              message: 'must be >= 0',
                                            },
                                          ];
                                          return false;
                                        }
                                      }
                                    }
                                  }
                                  var valid0 = _errs28 === errors;
                                } else {
                                  var valid0 = true;
                                }
                                if (valid0) {
                                  if (data.truncated !== undefined) {
                                    const _errs30 = errors;
                                    if (typeof data.truncated !== 'boolean') {
                                      validate110.errors = [
                                        {
                                          instancePath: instancePath + '/truncated',
                                          schemaPath: '#/properties/truncated/type',
                                          keyword: 'type',
                                          params: { type: 'boolean' },
                                          message: 'must be boolean',
                                        },
                                      ];
                                      return false;
                                    }
                                    var valid0 = _errs30 === errors;
                                  } else {
                                    var valid0 = true;
                                  }
                                  if (valid0) {
                                    if (data.upstream !== undefined) {
                                      let data16 = data.upstream;
                                      const _errs32 = errors;
                                      if (typeof data16 !== 'string' && data16 !== null) {
                                        validate110.errors = [
                                          {
                                            instancePath: instancePath + '/upstream',
                                            schemaPath: '#/properties/upstream/type',
                                            keyword: 'type',
                                            params: { type: schema35.properties.upstream.type },
                                            message: 'must be string,null',
                                          },
                                        ];
                                        return false;
                                      }
                                      var valid0 = _errs32 === errors;
                                    } else {
                                      var valid0 = true;
                                    }
                                  }
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate110.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate110.errors = vErrors;
  return errors === 0;
}
validate110.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate105(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate105.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.commit === undefined && (missing0 = 'commit')) ||
        (data.status === undefined && (missing0 = 'status')) ||
        (data.execution_notice === undefined && (missing0 = 'execution_notice'))
      ) {
        validate105.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'branch' ||
            key0 === 'commit' ||
            key0 === 'execution_notice' ||
            key0 === 'status'
          )) {
            validate105.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.branch !== undefined) {
            let data0 = data.branch;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate105.errors = [
                {
                  instancePath: instancePath + '/branch',
                  schemaPath: '#/properties/branch/type',
                  keyword: 'type',
                  params: { type: schema46.properties.branch.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.commit !== undefined) {
              const _errs4 = errors;
              if (
                !validate106(data.commit, {
                  instancePath: instancePath + '/commit',
                  parentData: data,
                  parentDataProperty: 'commit',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate106.errors : vErrors.concat(validate106.errors);
                errors = vErrors.length;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.execution_notice !== undefined) {
                const _errs5 = errors;
                if (typeof data.execution_notice !== 'string') {
                  validate105.errors = [
                    {
                      instancePath: instancePath + '/execution_notice',
                      schemaPath: '#/properties/execution_notice/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs5 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.status !== undefined) {
                  const _errs7 = errors;
                  if (
                    !validate110(data.status, {
                      instancePath: instancePath + '/status',
                      parentData: data,
                      parentDataProperty: 'status',
                      rootData,
                      dynamicAnchors,
                    })
                  ) {
                    vErrors =
                      vErrors === null ? validate110.errors : vErrors.concat(validate110.errors);
                    errors = vErrors.length;
                  }
                  var valid0 = _errs7 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate105.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate105.errors = vErrors;
  return errors === 0;
}
validate105.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema52 = {
  additionalProperties: false,
  properties: {
    branch: { type: 'string' },
    created: { type: 'boolean' },
    execution_notice: { type: 'string' },
    head: { type: ['string', 'null'] },
    previous_branch: { type: ['string', 'null'] },
    previous_head: { type: ['string', 'null'] },
    status: { $ref: '#/$defs/GitStatusResultV1' },
    switched: { type: 'boolean' },
  },
  required: ['branch', 'created', 'switched', 'status', 'execution_notice'],
  type: 'object',
};
function validate117(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate117.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.branch === undefined && (missing0 = 'branch')) ||
        (data.created === undefined && (missing0 = 'created')) ||
        (data.switched === undefined && (missing0 = 'switched')) ||
        (data.status === undefined && (missing0 = 'status')) ||
        (data.execution_notice === undefined && (missing0 = 'execution_notice'))
      ) {
        validate117.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'branch' ||
            key0 === 'created' ||
            key0 === 'execution_notice' ||
            key0 === 'head' ||
            key0 === 'previous_branch' ||
            key0 === 'previous_head' ||
            key0 === 'status' ||
            key0 === 'switched'
          )) {
            validate117.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.branch !== undefined) {
            const _errs2 = errors;
            if (typeof data.branch !== 'string') {
              validate117.errors = [
                {
                  instancePath: instancePath + '/branch',
                  schemaPath: '#/properties/branch/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.created !== undefined) {
              const _errs4 = errors;
              if (typeof data.created !== 'boolean') {
                validate117.errors = [
                  {
                    instancePath: instancePath + '/created',
                    schemaPath: '#/properties/created/type',
                    keyword: 'type',
                    params: { type: 'boolean' },
                    message: 'must be boolean',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.execution_notice !== undefined) {
                const _errs6 = errors;
                if (typeof data.execution_notice !== 'string') {
                  validate117.errors = [
                    {
                      instancePath: instancePath + '/execution_notice',
                      schemaPath: '#/properties/execution_notice/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.head !== undefined) {
                  let data3 = data.head;
                  const _errs8 = errors;
                  if (typeof data3 !== 'string' && data3 !== null) {
                    validate117.errors = [
                      {
                        instancePath: instancePath + '/head',
                        schemaPath: '#/properties/head/type',
                        keyword: 'type',
                        params: { type: schema52.properties.head.type },
                        message: 'must be string,null',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.previous_branch !== undefined) {
                    let data4 = data.previous_branch;
                    const _errs10 = errors;
                    if (typeof data4 !== 'string' && data4 !== null) {
                      validate117.errors = [
                        {
                          instancePath: instancePath + '/previous_branch',
                          schemaPath: '#/properties/previous_branch/type',
                          keyword: 'type',
                          params: { type: schema52.properties.previous_branch.type },
                          message: 'must be string,null',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs10 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.previous_head !== undefined) {
                      let data5 = data.previous_head;
                      const _errs12 = errors;
                      if (typeof data5 !== 'string' && data5 !== null) {
                        validate117.errors = [
                          {
                            instancePath: instancePath + '/previous_head',
                            schemaPath: '#/properties/previous_head/type',
                            keyword: 'type',
                            params: { type: schema52.properties.previous_head.type },
                            message: 'must be string,null',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs12 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.status !== undefined) {
                        const _errs14 = errors;
                        if (
                          !validate110(data.status, {
                            instancePath: instancePath + '/status',
                            parentData: data,
                            parentDataProperty: 'status',
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate110.errors
                              : vErrors.concat(validate110.errors);
                          errors = vErrors.length;
                        }
                        var valid0 = _errs14 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.switched !== undefined) {
                          const _errs15 = errors;
                          if (typeof data.switched !== 'boolean') {
                            validate117.errors = [
                              {
                                instancePath: instancePath + '/switched',
                                schemaPath: '#/properties/switched/type',
                                keyword: 'type',
                                params: { type: 'boolean' },
                                message: 'must be boolean',
                              },
                            ];
                            return false;
                          }
                          var valid0 = _errs15 === errors;
                        } else {
                          var valid0 = true;
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate117.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate117.errors = vErrors;
  return errors === 0;
}
validate117.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema53 = {
  oneOf: [
    {
      additionalProperties: false,
      properties: {
        kind: { const: 'fetch', type: 'string' },
        remote: { default: null, type: ['string', 'null'] },
      },
      required: ['kind'],
      type: 'object',
    },
    {
      additionalProperties: false,
      properties: {
        branch: { default: null, type: ['string', 'null'] },
        kind: { const: 'pull', type: 'string' },
        remote: { default: null, type: ['string', 'null'] },
      },
      required: ['kind'],
      type: 'object',
    },
  ],
  type: 'object',
};
function validate120(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate120.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (!(data && typeof data == 'object' && !Array.isArray(data))) {
    validate120.errors = [
      {
        instancePath,
        schemaPath: '#/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      },
    ];
    return false;
  }
  const _errs1 = errors;
  let valid0 = false;
  let passing0 = null;
  const _errs2 = errors;
  if (errors === _errs2) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (data.kind === undefined && (missing0 = 'kind')) {
        const err0 = {
          instancePath,
          schemaPath: '#/oneOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs4 = errors;
        for (const key0 in data) {
          if (!(key0 === 'kind' || key0 === 'remote')) {
            const err1 = {
              instancePath,
              schemaPath: '#/oneOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs4 === errors) {
          if (data.kind !== undefined) {
            let data0 = data.kind;
            const _errs5 = errors;
            if (typeof data0 !== 'string') {
              const err2 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/type',
                keyword: 'type',
                params: { type: 'string' },
                message: 'must be string',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            if ('fetch' !== data0) {
              const err3 = {
                instancePath: instancePath + '/kind',
                schemaPath: '#/oneOf/0/properties/kind/const',
                keyword: 'const',
                params: { allowedValue: 'fetch' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err3];
              } else {
                vErrors.push(err3);
              }
              errors++;
            }
            var valid1 = _errs5 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.remote !== undefined) {
              let data1 = data.remote;
              const _errs7 = errors;
              if (typeof data1 !== 'string' && data1 !== null) {
                const err4 = {
                  instancePath: instancePath + '/remote',
                  schemaPath: '#/oneOf/0/properties/remote/type',
                  keyword: 'type',
                  params: { type: schema53.oneOf[0].properties.remote.type },
                  message: 'must be string,null',
                };
                if (vErrors === null) {
                  vErrors = [err4];
                } else {
                  vErrors.push(err4);
                }
                errors++;
              }
              var valid1 = _errs7 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err5 = {
        instancePath,
        schemaPath: '#/oneOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err5];
      } else {
        vErrors.push(err5);
      }
      errors++;
    }
  }
  var _valid0 = _errs2 === errors;
  if (_valid0) {
    valid0 = true;
    passing0 = 0;
    var props0 = true;
  }
  const _errs9 = errors;
  if (errors === _errs9) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (data.kind === undefined && (missing1 = 'kind')) {
        const err6 = {
          instancePath,
          schemaPath: '#/oneOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err6];
        } else {
          vErrors.push(err6);
        }
        errors++;
      } else {
        const _errs11 = errors;
        for (const key1 in data) {
          if (!(key1 === 'branch' || key1 === 'kind' || key1 === 'remote')) {
            const err7 = {
              instancePath,
              schemaPath: '#/oneOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err7];
            } else {
              vErrors.push(err7);
            }
            errors++;
            break;
          }
        }
        if (_errs11 === errors) {
          if (data.branch !== undefined) {
            let data2 = data.branch;
            const _errs12 = errors;
            if (typeof data2 !== 'string' && data2 !== null) {
              const err8 = {
                instancePath: instancePath + '/branch',
                schemaPath: '#/oneOf/1/properties/branch/type',
                keyword: 'type',
                params: { type: schema53.oneOf[1].properties.branch.type },
                message: 'must be string,null',
              };
              if (vErrors === null) {
                vErrors = [err8];
              } else {
                vErrors.push(err8);
              }
              errors++;
            }
            var valid2 = _errs12 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.kind !== undefined) {
              let data3 = data.kind;
              const _errs14 = errors;
              if (typeof data3 !== 'string') {
                const err9 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/1/properties/kind/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                };
                if (vErrors === null) {
                  vErrors = [err9];
                } else {
                  vErrors.push(err9);
                }
                errors++;
              }
              if ('pull' !== data3) {
                const err10 = {
                  instancePath: instancePath + '/kind',
                  schemaPath: '#/oneOf/1/properties/kind/const',
                  keyword: 'const',
                  params: { allowedValue: 'pull' },
                  message: 'must be equal to constant',
                };
                if (vErrors === null) {
                  vErrors = [err10];
                } else {
                  vErrors.push(err10);
                }
                errors++;
              }
              var valid2 = _errs14 === errors;
            } else {
              var valid2 = true;
            }
            if (valid2) {
              if (data.remote !== undefined) {
                let data4 = data.remote;
                const _errs16 = errors;
                if (typeof data4 !== 'string' && data4 !== null) {
                  const err11 = {
                    instancePath: instancePath + '/remote',
                    schemaPath: '#/oneOf/1/properties/remote/type',
                    keyword: 'type',
                    params: { type: schema53.oneOf[1].properties.remote.type },
                    message: 'must be string,null',
                  };
                  if (vErrors === null) {
                    vErrors = [err11];
                  } else {
                    vErrors.push(err11);
                  }
                  errors++;
                }
                var valid2 = _errs16 === errors;
              } else {
                var valid2 = true;
              }
            }
          }
        }
      }
    } else {
      const err12 = {
        instancePath,
        schemaPath: '#/oneOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err12];
      } else {
        vErrors.push(err12);
      }
      errors++;
    }
  }
  var _valid0 = _errs9 === errors;
  if (_valid0 && valid0) {
    valid0 = false;
    passing0 = [passing0, 1];
  } else {
    if (_valid0) {
      valid0 = true;
      passing0 = 1;
      if (props0 !== true) {
        props0 = true;
      }
    }
  }
  if (!valid0) {
    const err13 = {
      instancePath,
      schemaPath: '#/oneOf',
      keyword: 'oneOf',
      params: { passingSchemas: passing0 },
      message: 'must match exactly one schema in oneOf',
    };
    if (vErrors === null) {
      vErrors = [err13];
    } else {
      vErrors.push(err13);
    }
    errors++;
    validate120.errors = vErrors;
    return false;
  } else {
    errors = _errs1;
    if (vErrors !== null) {
      if (_errs1) {
        vErrors.length = _errs1;
      } else {
        vErrors = null;
      }
    }
  }
  validate120.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate120.evaluated = { dynamicProps: true, dynamicItems: false };
const schema54 = {
  additionalProperties: false,
  properties: {
    branch: { default: null, type: ['string', 'null'] },
    force_with_lease: {
      default: null,
      description:
        'Full expected remote object ID. An exact lease, never a bare force or tracking-ref lease.',
      type: ['string', 'null'],
    },
    remote: { default: null, type: ['string', 'null'] },
    remote_branch: { default: null, type: ['string', 'null'] },
    set_upstream: { default: false, type: 'boolean' },
  },
  type: 'object',
};
function validate122(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate122.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      const _errs1 = errors;
      for (const key0 in data) {
        if (!(
          key0 === 'branch' ||
          key0 === 'force_with_lease' ||
          key0 === 'remote' ||
          key0 === 'remote_branch' ||
          key0 === 'set_upstream'
        )) {
          validate122.errors = [
            {
              instancePath,
              schemaPath: '#/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            },
          ];
          return false;
          break;
        }
      }
      if (_errs1 === errors) {
        if (data.branch !== undefined) {
          let data0 = data.branch;
          const _errs2 = errors;
          if (typeof data0 !== 'string' && data0 !== null) {
            validate122.errors = [
              {
                instancePath: instancePath + '/branch',
                schemaPath: '#/properties/branch/type',
                keyword: 'type',
                params: { type: schema54.properties.branch.type },
                message: 'must be string,null',
              },
            ];
            return false;
          }
          var valid0 = _errs2 === errors;
        } else {
          var valid0 = true;
        }
        if (valid0) {
          if (data.force_with_lease !== undefined) {
            let data1 = data.force_with_lease;
            const _errs4 = errors;
            if (typeof data1 !== 'string' && data1 !== null) {
              validate122.errors = [
                {
                  instancePath: instancePath + '/force_with_lease',
                  schemaPath: '#/properties/force_with_lease/type',
                  keyword: 'type',
                  params: { type: schema54.properties.force_with_lease.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs4 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.remote !== undefined) {
              let data2 = data.remote;
              const _errs6 = errors;
              if (typeof data2 !== 'string' && data2 !== null) {
                validate122.errors = [
                  {
                    instancePath: instancePath + '/remote',
                    schemaPath: '#/properties/remote/type',
                    keyword: 'type',
                    params: { type: schema54.properties.remote.type },
                    message: 'must be string,null',
                  },
                ];
                return false;
              }
              var valid0 = _errs6 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.remote_branch !== undefined) {
                let data3 = data.remote_branch;
                const _errs8 = errors;
                if (typeof data3 !== 'string' && data3 !== null) {
                  validate122.errors = [
                    {
                      instancePath: instancePath + '/remote_branch',
                      schemaPath: '#/properties/remote_branch/type',
                      keyword: 'type',
                      params: { type: schema54.properties.remote_branch.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs8 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.set_upstream !== undefined) {
                  const _errs10 = errors;
                  if (typeof data.set_upstream !== 'boolean') {
                    validate122.errors = [
                      {
                        instancePath: instancePath + '/set_upstream',
                        schemaPath: '#/properties/set_upstream/type',
                        keyword: 'type',
                        params: { type: 'boolean' },
                        message: 'must be boolean',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs10 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate122.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate122.errors = vErrors;
  return errors === 0;
}
validate122.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
const schema55 = {
  additionalProperties: false,
  properties: {
    execution_notice: { type: 'string' },
    kind: { type: 'string' },
    local_branch: { type: ['string', 'null'] },
    refs: { items: { $ref: '#/$defs/GitRefUpdateV1' }, type: 'array' },
    remote: { type: 'string' },
    remote_branch: { type: ['string', 'null'] },
    remote_url: {
      description: 'Normalized display and consent URL; has no userinfo, query or fragment.',
      type: 'string',
    },
    status: { $ref: '#/$defs/GitStatusResultV1' },
  },
  required: ['kind', 'remote', 'remote_url', 'refs', 'status', 'execution_notice'],
  type: 'object',
};
const schema56 = {
  additionalProperties: false,
  properties: {
    after: { type: ['string', 'null'] },
    before: { type: ['string', 'null'] },
    reference: { type: 'string' },
    result: { type: 'string' },
  },
  required: ['reference', 'result'],
  type: 'object',
};
function validate125(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate125.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.reference === undefined && (missing0 = 'reference')) ||
        (data.result === undefined && (missing0 = 'result'))
      ) {
        validate125.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'after' ||
            key0 === 'before' ||
            key0 === 'reference' ||
            key0 === 'result'
          )) {
            validate125.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.after !== undefined) {
            let data0 = data.after;
            const _errs2 = errors;
            if (typeof data0 !== 'string' && data0 !== null) {
              validate125.errors = [
                {
                  instancePath: instancePath + '/after',
                  schemaPath: '#/properties/after/type',
                  keyword: 'type',
                  params: { type: schema56.properties.after.type },
                  message: 'must be string,null',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.before !== undefined) {
              let data1 = data.before;
              const _errs4 = errors;
              if (typeof data1 !== 'string' && data1 !== null) {
                validate125.errors = [
                  {
                    instancePath: instancePath + '/before',
                    schemaPath: '#/properties/before/type',
                    keyword: 'type',
                    params: { type: schema56.properties.before.type },
                    message: 'must be string,null',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.reference !== undefined) {
                const _errs6 = errors;
                if (typeof data.reference !== 'string') {
                  validate125.errors = [
                    {
                      instancePath: instancePath + '/reference',
                      schemaPath: '#/properties/reference/type',
                      keyword: 'type',
                      params: { type: 'string' },
                      message: 'must be string',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.result !== undefined) {
                  const _errs8 = errors;
                  if (typeof data.result !== 'string') {
                    validate125.errors = [
                      {
                        instancePath: instancePath + '/result',
                        schemaPath: '#/properties/result/type',
                        keyword: 'type',
                        params: { type: 'string' },
                        message: 'must be string',
                      },
                    ];
                    return false;
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
              }
            }
          }
        }
      }
    } else {
      validate125.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate125.errors = vErrors;
  return errors === 0;
}
validate125.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate124(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate124.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  if (errors === 0) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.kind === undefined && (missing0 = 'kind')) ||
        (data.remote === undefined && (missing0 = 'remote')) ||
        (data.remote_url === undefined && (missing0 = 'remote_url')) ||
        (data.refs === undefined && (missing0 = 'refs')) ||
        (data.status === undefined && (missing0 = 'status')) ||
        (data.execution_notice === undefined && (missing0 = 'execution_notice'))
      ) {
        validate124.errors = [
          {
            instancePath,
            schemaPath: '#/required',
            keyword: 'required',
            params: { missingProperty: missing0 },
            message: "must have required property '" + missing0 + "'",
          },
        ];
        return false;
      } else {
        const _errs1 = errors;
        for (const key0 in data) {
          if (!(
            key0 === 'execution_notice' ||
            key0 === 'kind' ||
            key0 === 'local_branch' ||
            key0 === 'refs' ||
            key0 === 'remote' ||
            key0 === 'remote_branch' ||
            key0 === 'remote_url' ||
            key0 === 'status'
          )) {
            validate124.errors = [
              {
                instancePath,
                schemaPath: '#/additionalProperties',
                keyword: 'additionalProperties',
                params: { additionalProperty: key0 },
                message: 'must NOT have additional properties',
              },
            ];
            return false;
            break;
          }
        }
        if (_errs1 === errors) {
          if (data.execution_notice !== undefined) {
            const _errs2 = errors;
            if (typeof data.execution_notice !== 'string') {
              validate124.errors = [
                {
                  instancePath: instancePath + '/execution_notice',
                  schemaPath: '#/properties/execution_notice/type',
                  keyword: 'type',
                  params: { type: 'string' },
                  message: 'must be string',
                },
              ];
              return false;
            }
            var valid0 = _errs2 === errors;
          } else {
            var valid0 = true;
          }
          if (valid0) {
            if (data.kind !== undefined) {
              const _errs4 = errors;
              if (typeof data.kind !== 'string') {
                validate124.errors = [
                  {
                    instancePath: instancePath + '/kind',
                    schemaPath: '#/properties/kind/type',
                    keyword: 'type',
                    params: { type: 'string' },
                    message: 'must be string',
                  },
                ];
                return false;
              }
              var valid0 = _errs4 === errors;
            } else {
              var valid0 = true;
            }
            if (valid0) {
              if (data.local_branch !== undefined) {
                let data2 = data.local_branch;
                const _errs6 = errors;
                if (typeof data2 !== 'string' && data2 !== null) {
                  validate124.errors = [
                    {
                      instancePath: instancePath + '/local_branch',
                      schemaPath: '#/properties/local_branch/type',
                      keyword: 'type',
                      params: { type: schema55.properties.local_branch.type },
                      message: 'must be string,null',
                    },
                  ];
                  return false;
                }
                var valid0 = _errs6 === errors;
              } else {
                var valid0 = true;
              }
              if (valid0) {
                if (data.refs !== undefined) {
                  let data3 = data.refs;
                  const _errs8 = errors;
                  if (errors === _errs8) {
                    if (Array.isArray(data3)) {
                      var valid1 = true;
                      const len0 = data3.length;
                      for (let i0 = 0; i0 < len0; i0++) {
                        const _errs10 = errors;
                        if (
                          !validate125(data3[i0], {
                            instancePath: instancePath + '/refs/' + i0,
                            parentData: data3,
                            parentDataProperty: i0,
                            rootData,
                            dynamicAnchors,
                          })
                        ) {
                          vErrors =
                            vErrors === null
                              ? validate125.errors
                              : vErrors.concat(validate125.errors);
                          errors = vErrors.length;
                        }
                        var valid1 = _errs10 === errors;
                        if (!valid1) {
                          break;
                        }
                      }
                    } else {
                      validate124.errors = [
                        {
                          instancePath: instancePath + '/refs',
                          schemaPath: '#/properties/refs/type',
                          keyword: 'type',
                          params: { type: 'array' },
                          message: 'must be array',
                        },
                      ];
                      return false;
                    }
                  }
                  var valid0 = _errs8 === errors;
                } else {
                  var valid0 = true;
                }
                if (valid0) {
                  if (data.remote !== undefined) {
                    const _errs11 = errors;
                    if (typeof data.remote !== 'string') {
                      validate124.errors = [
                        {
                          instancePath: instancePath + '/remote',
                          schemaPath: '#/properties/remote/type',
                          keyword: 'type',
                          params: { type: 'string' },
                          message: 'must be string',
                        },
                      ];
                      return false;
                    }
                    var valid0 = _errs11 === errors;
                  } else {
                    var valid0 = true;
                  }
                  if (valid0) {
                    if (data.remote_branch !== undefined) {
                      let data6 = data.remote_branch;
                      const _errs13 = errors;
                      if (typeof data6 !== 'string' && data6 !== null) {
                        validate124.errors = [
                          {
                            instancePath: instancePath + '/remote_branch',
                            schemaPath: '#/properties/remote_branch/type',
                            keyword: 'type',
                            params: { type: schema55.properties.remote_branch.type },
                            message: 'must be string,null',
                          },
                        ];
                        return false;
                      }
                      var valid0 = _errs13 === errors;
                    } else {
                      var valid0 = true;
                    }
                    if (valid0) {
                      if (data.remote_url !== undefined) {
                        const _errs15 = errors;
                        if (typeof data.remote_url !== 'string') {
                          validate124.errors = [
                            {
                              instancePath: instancePath + '/remote_url',
                              schemaPath: '#/properties/remote_url/type',
                              keyword: 'type',
                              params: { type: 'string' },
                              message: 'must be string',
                            },
                          ];
                          return false;
                        }
                        var valid0 = _errs15 === errors;
                      } else {
                        var valid0 = true;
                      }
                      if (valid0) {
                        if (data.status !== undefined) {
                          const _errs17 = errors;
                          if (
                            !validate110(data.status, {
                              instancePath: instancePath + '/status',
                              parentData: data,
                              parentDataProperty: 'status',
                              rootData,
                              dynamicAnchors,
                            })
                          ) {
                            vErrors =
                              vErrors === null
                                ? validate110.errors
                                : vErrors.concat(validate110.errors);
                            errors = vErrors.length;
                          }
                          var valid0 = _errs17 === errors;
                        } else {
                          var valid0 = true;
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    } else {
      validate124.errors = [
        {
          instancePath,
          schemaPath: '#/type',
          keyword: 'type',
          params: { type: 'object' },
          message: 'must be object',
        },
      ];
      return false;
    }
  }
  validate124.errors = vErrors;
  return errors === 0;
}
validate124.evaluated = { props: true, dynamicProps: false, dynamicItems: false };
function validate100(
  data,
  { instancePath = '', parentData, parentDataProperty, rootData = data, dynamicAnchors = {} } = {},
) {
  let vErrors = null;
  let errors = 0;
  const evaluated0 = validate100.evaluated;
  if (evaluated0.dynamicProps) {
    evaluated0.props = undefined;
  }
  if (evaluated0.dynamicItems) {
    evaluated0.items = undefined;
  }
  const _errs0 = errors;
  let valid0 = false;
  const _errs1 = errors;
  if (errors === _errs1) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing0;
      if (
        (data.command === undefined && (missing0 = 'command')) ||
        (data.value === undefined && (missing0 = 'value'))
      ) {
        const err0 = {
          instancePath,
          schemaPath: '#/anyOf/0/required',
          keyword: 'required',
          params: { missingProperty: missing0 },
          message: "must have required property '" + missing0 + "'",
        };
        if (vErrors === null) {
          vErrors = [err0];
        } else {
          vErrors.push(err0);
        }
        errors++;
      } else {
        const _errs3 = errors;
        for (const key0 in data) {
          if (!(key0 === 'command' || key0 === 'value')) {
            const err1 = {
              instancePath,
              schemaPath: '#/anyOf/0/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key0 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err1];
            } else {
              vErrors.push(err1);
            }
            errors++;
            break;
          }
        }
        if (_errs3 === errors) {
          if (data.command !== undefined) {
            const _errs4 = errors;
            if ('git.commit.input' !== data.command) {
              const err2 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/0/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.commit.input' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err2];
              } else {
                vErrors.push(err2);
              }
              errors++;
            }
            var valid1 = _errs4 === errors;
          } else {
            var valid1 = true;
          }
          if (valid1) {
            if (data.value !== undefined) {
              const _errs5 = errors;
              if (
                !validate101(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate101.errors : vErrors.concat(validate101.errors);
                errors = vErrors.length;
              }
              var valid1 = _errs5 === errors;
            } else {
              var valid1 = true;
            }
          }
        }
      }
    } else {
      const err3 = {
        instancePath,
        schemaPath: '#/anyOf/0/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err3];
      } else {
        vErrors.push(err3);
      }
      errors++;
    }
  }
  var _valid0 = _errs1 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    var props0 = true;
  }
  const _errs6 = errors;
  if (errors === _errs6) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing1;
      if (
        (data.command === undefined && (missing1 = 'command')) ||
        (data.value === undefined && (missing1 = 'value'))
      ) {
        const err4 = {
          instancePath,
          schemaPath: '#/anyOf/1/required',
          keyword: 'required',
          params: { missingProperty: missing1 },
          message: "must have required property '" + missing1 + "'",
        };
        if (vErrors === null) {
          vErrors = [err4];
        } else {
          vErrors.push(err4);
        }
        errors++;
      } else {
        const _errs8 = errors;
        for (const key1 in data) {
          if (!(key1 === 'command' || key1 === 'value')) {
            const err5 = {
              instancePath,
              schemaPath: '#/anyOf/1/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key1 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err5];
            } else {
              vErrors.push(err5);
            }
            errors++;
            break;
          }
        }
        if (_errs8 === errors) {
          if (data.command !== undefined) {
            const _errs9 = errors;
            if ('git.branch.input' !== data.command) {
              const err6 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/1/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.branch.input' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err6];
              } else {
                vErrors.push(err6);
              }
              errors++;
            }
            var valid2 = _errs9 === errors;
          } else {
            var valid2 = true;
          }
          if (valid2) {
            if (data.value !== undefined) {
              const _errs10 = errors;
              if (
                !validate103(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate103.errors : vErrors.concat(validate103.errors);
                errors = vErrors.length;
              }
              var valid2 = _errs10 === errors;
            } else {
              var valid2 = true;
            }
          }
        }
      }
    } else {
      const err7 = {
        instancePath,
        schemaPath: '#/anyOf/1/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err7];
      } else {
        vErrors.push(err7);
      }
      errors++;
    }
  }
  var _valid0 = _errs6 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs11 = errors;
  if (errors === _errs11) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing2;
      if (
        (data.command === undefined && (missing2 = 'command')) ||
        (data.value === undefined && (missing2 = 'value'))
      ) {
        const err8 = {
          instancePath,
          schemaPath: '#/anyOf/2/required',
          keyword: 'required',
          params: { missingProperty: missing2 },
          message: "must have required property '" + missing2 + "'",
        };
        if (vErrors === null) {
          vErrors = [err8];
        } else {
          vErrors.push(err8);
        }
        errors++;
      } else {
        const _errs13 = errors;
        for (const key2 in data) {
          if (!(key2 === 'command' || key2 === 'value')) {
            const err9 = {
              instancePath,
              schemaPath: '#/anyOf/2/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key2 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err9];
            } else {
              vErrors.push(err9);
            }
            errors++;
            break;
          }
        }
        if (_errs13 === errors) {
          if (data.command !== undefined) {
            const _errs14 = errors;
            if ('git.commit.result' !== data.command) {
              const err10 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/2/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.commit.result' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err10];
              } else {
                vErrors.push(err10);
              }
              errors++;
            }
            var valid3 = _errs14 === errors;
          } else {
            var valid3 = true;
          }
          if (valid3) {
            if (data.value !== undefined) {
              const _errs15 = errors;
              if (
                !validate105(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate105.errors : vErrors.concat(validate105.errors);
                errors = vErrors.length;
              }
              var valid3 = _errs15 === errors;
            } else {
              var valid3 = true;
            }
          }
        }
      }
    } else {
      const err11 = {
        instancePath,
        schemaPath: '#/anyOf/2/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err11];
      } else {
        vErrors.push(err11);
      }
      errors++;
    }
  }
  var _valid0 = _errs11 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs16 = errors;
  if (errors === _errs16) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing3;
      if (
        (data.command === undefined && (missing3 = 'command')) ||
        (data.value === undefined && (missing3 = 'value'))
      ) {
        const err12 = {
          instancePath,
          schemaPath: '#/anyOf/3/required',
          keyword: 'required',
          params: { missingProperty: missing3 },
          message: "must have required property '" + missing3 + "'",
        };
        if (vErrors === null) {
          vErrors = [err12];
        } else {
          vErrors.push(err12);
        }
        errors++;
      } else {
        const _errs18 = errors;
        for (const key3 in data) {
          if (!(key3 === 'command' || key3 === 'value')) {
            const err13 = {
              instancePath,
              schemaPath: '#/anyOf/3/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key3 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err13];
            } else {
              vErrors.push(err13);
            }
            errors++;
            break;
          }
        }
        if (_errs18 === errors) {
          if (data.command !== undefined) {
            const _errs19 = errors;
            if ('git.branch.result' !== data.command) {
              const err14 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/3/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.branch.result' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err14];
              } else {
                vErrors.push(err14);
              }
              errors++;
            }
            var valid4 = _errs19 === errors;
          } else {
            var valid4 = true;
          }
          if (valid4) {
            if (data.value !== undefined) {
              const _errs20 = errors;
              if (
                !validate117(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate117.errors : vErrors.concat(validate117.errors);
                errors = vErrors.length;
              }
              var valid4 = _errs20 === errors;
            } else {
              var valid4 = true;
            }
          }
        }
      }
    } else {
      const err15 = {
        instancePath,
        schemaPath: '#/anyOf/3/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err15];
      } else {
        vErrors.push(err15);
      }
      errors++;
    }
  }
  var _valid0 = _errs16 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs21 = errors;
  if (errors === _errs21) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing4;
      if (
        (data.command === undefined && (missing4 = 'command')) ||
        (data.value === undefined && (missing4 = 'value'))
      ) {
        const err16 = {
          instancePath,
          schemaPath: '#/anyOf/4/required',
          keyword: 'required',
          params: { missingProperty: missing4 },
          message: "must have required property '" + missing4 + "'",
        };
        if (vErrors === null) {
          vErrors = [err16];
        } else {
          vErrors.push(err16);
        }
        errors++;
      } else {
        const _errs23 = errors;
        for (const key4 in data) {
          if (!(key4 === 'command' || key4 === 'value')) {
            const err17 = {
              instancePath,
              schemaPath: '#/anyOf/4/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key4 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err17];
            } else {
              vErrors.push(err17);
            }
            errors++;
            break;
          }
        }
        if (_errs23 === errors) {
          if (data.command !== undefined) {
            const _errs24 = errors;
            if ('git.sync.input' !== data.command) {
              const err18 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/4/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.sync.input' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err18];
              } else {
                vErrors.push(err18);
              }
              errors++;
            }
            var valid5 = _errs24 === errors;
          } else {
            var valid5 = true;
          }
          if (valid5) {
            if (data.value !== undefined) {
              const _errs25 = errors;
              if (
                !validate120(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate120.errors : vErrors.concat(validate120.errors);
                errors = vErrors.length;
              }
              var valid5 = _errs25 === errors;
            } else {
              var valid5 = true;
            }
          }
        }
      }
    } else {
      const err19 = {
        instancePath,
        schemaPath: '#/anyOf/4/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err19];
      } else {
        vErrors.push(err19);
      }
      errors++;
    }
  }
  var _valid0 = _errs21 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs26 = errors;
  if (errors === _errs26) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing5;
      if (
        (data.command === undefined && (missing5 = 'command')) ||
        (data.value === undefined && (missing5 = 'value'))
      ) {
        const err20 = {
          instancePath,
          schemaPath: '#/anyOf/5/required',
          keyword: 'required',
          params: { missingProperty: missing5 },
          message: "must have required property '" + missing5 + "'",
        };
        if (vErrors === null) {
          vErrors = [err20];
        } else {
          vErrors.push(err20);
        }
        errors++;
      } else {
        const _errs28 = errors;
        for (const key5 in data) {
          if (!(key5 === 'command' || key5 === 'value')) {
            const err21 = {
              instancePath,
              schemaPath: '#/anyOf/5/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key5 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err21];
            } else {
              vErrors.push(err21);
            }
            errors++;
            break;
          }
        }
        if (_errs28 === errors) {
          if (data.command !== undefined) {
            const _errs29 = errors;
            if ('git.push.input' !== data.command) {
              const err22 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/5/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.push.input' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err22];
              } else {
                vErrors.push(err22);
              }
              errors++;
            }
            var valid6 = _errs29 === errors;
          } else {
            var valid6 = true;
          }
          if (valid6) {
            if (data.value !== undefined) {
              const _errs30 = errors;
              if (
                !validate122(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate122.errors : vErrors.concat(validate122.errors);
                errors = vErrors.length;
              }
              var valid6 = _errs30 === errors;
            } else {
              var valid6 = true;
            }
          }
        }
      }
    } else {
      const err23 = {
        instancePath,
        schemaPath: '#/anyOf/5/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err23];
      } else {
        vErrors.push(err23);
      }
      errors++;
    }
  }
  var _valid0 = _errs26 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs31 = errors;
  if (errors === _errs31) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing6;
      if (
        (data.command === undefined && (missing6 = 'command')) ||
        (data.value === undefined && (missing6 = 'value'))
      ) {
        const err24 = {
          instancePath,
          schemaPath: '#/anyOf/6/required',
          keyword: 'required',
          params: { missingProperty: missing6 },
          message: "must have required property '" + missing6 + "'",
        };
        if (vErrors === null) {
          vErrors = [err24];
        } else {
          vErrors.push(err24);
        }
        errors++;
      } else {
        const _errs33 = errors;
        for (const key6 in data) {
          if (!(key6 === 'command' || key6 === 'value')) {
            const err25 = {
              instancePath,
              schemaPath: '#/anyOf/6/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key6 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err25];
            } else {
              vErrors.push(err25);
            }
            errors++;
            break;
          }
        }
        if (_errs33 === errors) {
          if (data.command !== undefined) {
            const _errs34 = errors;
            if ('git.sync.result' !== data.command) {
              const err26 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/6/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.sync.result' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err26];
              } else {
                vErrors.push(err26);
              }
              errors++;
            }
            var valid7 = _errs34 === errors;
          } else {
            var valid7 = true;
          }
          if (valid7) {
            if (data.value !== undefined) {
              const _errs35 = errors;
              if (
                !validate124(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate124.errors : vErrors.concat(validate124.errors);
                errors = vErrors.length;
              }
              var valid7 = _errs35 === errors;
            } else {
              var valid7 = true;
            }
          }
        }
      }
    } else {
      const err27 = {
        instancePath,
        schemaPath: '#/anyOf/6/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err27];
      } else {
        vErrors.push(err27);
      }
      errors++;
    }
  }
  var _valid0 = _errs31 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  const _errs36 = errors;
  if (errors === _errs36) {
    if (data && typeof data == 'object' && !Array.isArray(data)) {
      let missing7;
      if (
        (data.command === undefined && (missing7 = 'command')) ||
        (data.value === undefined && (missing7 = 'value'))
      ) {
        const err28 = {
          instancePath,
          schemaPath: '#/anyOf/7/required',
          keyword: 'required',
          params: { missingProperty: missing7 },
          message: "must have required property '" + missing7 + "'",
        };
        if (vErrors === null) {
          vErrors = [err28];
        } else {
          vErrors.push(err28);
        }
        errors++;
      } else {
        const _errs38 = errors;
        for (const key7 in data) {
          if (!(key7 === 'command' || key7 === 'value')) {
            const err29 = {
              instancePath,
              schemaPath: '#/anyOf/7/additionalProperties',
              keyword: 'additionalProperties',
              params: { additionalProperty: key7 },
              message: 'must NOT have additional properties',
            };
            if (vErrors === null) {
              vErrors = [err29];
            } else {
              vErrors.push(err29);
            }
            errors++;
            break;
          }
        }
        if (_errs38 === errors) {
          if (data.command !== undefined) {
            const _errs39 = errors;
            if ('git.push.result' !== data.command) {
              const err30 = {
                instancePath: instancePath + '/command',
                schemaPath: '#/anyOf/7/properties/command/const',
                keyword: 'const',
                params: { allowedValue: 'git.push.result' },
                message: 'must be equal to constant',
              };
              if (vErrors === null) {
                vErrors = [err30];
              } else {
                vErrors.push(err30);
              }
              errors++;
            }
            var valid8 = _errs39 === errors;
          } else {
            var valid8 = true;
          }
          if (valid8) {
            if (data.value !== undefined) {
              const _errs40 = errors;
              if (
                !validate124(data.value, {
                  instancePath: instancePath + '/value',
                  parentData: data,
                  parentDataProperty: 'value',
                  rootData,
                  dynamicAnchors,
                })
              ) {
                vErrors =
                  vErrors === null ? validate124.errors : vErrors.concat(validate124.errors);
                errors = vErrors.length;
              }
              var valid8 = _errs40 === errors;
            } else {
              var valid8 = true;
            }
          }
        }
      }
    } else {
      const err31 = {
        instancePath,
        schemaPath: '#/anyOf/7/type',
        keyword: 'type',
        params: { type: 'object' },
        message: 'must be object',
      };
      if (vErrors === null) {
        vErrors = [err31];
      } else {
        vErrors.push(err31);
      }
      errors++;
    }
  }
  var _valid0 = _errs36 === errors;
  valid0 = valid0 || _valid0;
  if (_valid0) {
    if (props0 !== true) {
      props0 = true;
    }
  }
  if (!valid0) {
    const err32 = {
      instancePath,
      schemaPath: '#/anyOf',
      keyword: 'anyOf',
      params: {},
      message: 'must match a schema in anyOf',
    };
    if (vErrors === null) {
      vErrors = [err32];
    } else {
      vErrors.push(err32);
    }
    errors++;
    validate100.errors = vErrors;
    return false;
  } else {
    errors = _errs0;
    if (vErrors !== null) {
      if (_errs0) {
        vErrors.length = _errs0;
      } else {
        vErrors = null;
      }
    }
  }
  validate100.errors = vErrors;
  evaluated0.props = props0;
  return errors === 0;
}
validate100.evaluated = { dynamicProps: true, dynamicItems: false };
