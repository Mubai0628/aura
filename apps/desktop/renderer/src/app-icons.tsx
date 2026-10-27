import type { SVGProps } from 'react';

export type IconName =
  | 'activity'
  | 'archive'
  | 'arrow-left'
  | 'arrow-right'
  | 'bell'
  | 'book'
  | 'bookmark'
  | 'browser'
  | 'chat'
  | 'check'
  | 'chevron-down'
  | 'chevron-right'
  | 'close'
  | 'code'
  | 'expand'
  | 'file'
  | 'folder'
  | 'hand'
  | 'home'
  | 'eye'
  | 'eye-off'
  | 'model'
  | 'plug'
  | 'minus'
  | 'more'
  | 'plus'
  | 'puzzle'
  | 'review'
  | 'refresh'
  | 'search'
  | 'send'
  | 'settings'
  | 'shield'
  | 'shrink'
  | 'slash'
  | 'sidebar-left'
  | 'sidebar-right'
  | 'spark'
  | 'terminal'
  | 'warning'
  | 'workspace'
  | 'wrench';

interface AppIconProps extends Omit<SVGProps<SVGSVGElement>, 'children' | 'name'> {
  name: IconName;
  size?: number;
}

export function AppIcon({ name, size = 16, ...props }: AppIconProps) {
  const paths = iconPaths[name];

  return (
    <svg aria-hidden="true" fill="none" height={size} viewBox="0 0 24 24" width={size} {...props}>
      {paths}
    </svg>
  );
}

const strokeProps = {
  stroke: 'currentColor',
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  strokeWidth: 1.7,
};

const iconPaths: Record<IconName, React.ReactNode> = {
  activity: (
    <>
      <path d="M4 12h3l2-6 4 12 2-6h5" {...strokeProps} />
      <circle cx="4" cy="12" r="1" fill="currentColor" />
    </>
  ),
  archive: (
    <>
      <rect height="4" rx="1" width="17" x="3.5" y="4.5" {...strokeProps} />
      <path d="M5.5 8.5v10.5h13V8.5M10 12.5h4" {...strokeProps} />
    </>
  ),
  'arrow-left': <path d="m11 6-6 6 6 6M5 12h14" {...strokeProps} />,
  'arrow-right': <path d="m13 6 6 6-6 6M19 12H5" {...strokeProps} />,
  browser: (
    <>
      <circle cx="12" cy="12" r="8.5" {...strokeProps} />
      <path
        d="M3.8 12h16.4M12 3.5c2.1 2.3 3.2 5.1 3.2 8.5S14.1 18.2 12 20.5M12 3.5C9.9 5.8 8.8 8.6 8.8 12s1.1 6.2 3.2 8.5"
        {...strokeProps}
      />
    </>
  ),
  chat: <path d="M5.2 5.5h13.6v10H11l-4.5 3v-3H5.2z" {...strokeProps} />,
  check: <path d="m5.5 12 4 4 9-9" {...strokeProps} />,
  minus: <path d="M7 12h10" {...strokeProps} />,
  'chevron-down': <path d="m7 9.5 5 5 5-5" {...strokeProps} />,
  'chevron-right': <path d="m9.5 7 5 5-5 5" {...strokeProps} />,
  close: <path d="m7 7 10 10M17 7 7 17" {...strokeProps} />,
  code: <path d="m9 7-5 5 5 5M15 7l5 5-5 5M13.5 5 10.5 19" {...strokeProps} />,
  expand: <path d="M14 5h5v5M10 19H5v-5M19 5l-6 6M5 19l6-6" {...strokeProps} />,
  shrink: <path d="M19 10h-5V5M5 14h5v5M14 10l6-6M10 14l-6 6" {...strokeProps} />,
  file: (
    <>
      <path d="M6.5 3.5h7l4 4v13h-11z" {...strokeProps} />
      <path d="M13.5 3.5v4h4" {...strokeProps} />
    </>
  ),
  eye: (
    <>
      <path
        d="M3.5 12s3.2-5.2 8.5-5.2 8.5 5.2 8.5 5.2-3.2 5.2-8.5 5.2S3.5 12 3.5 12Z"
        {...strokeProps}
      />
      <circle cx="12" cy="12" r="2.4" {...strokeProps} />
    </>
  ),
  'eye-off': (
    <>
      <path
        d="M4.4 8.3C3.8 9 3.5 9.7 3.5 9.7S6.7 15 12 15c1 0 1.9-.2 2.7-.5M7.4 5.9A9.5 9.5 0 0 1 12 4.5c5.3 0 8.5 5.2 8.5 5.2s-.7 1.2-2 2.4M9.8 7.7A3 3 0 0 1 14 11.9M4 3.5l16 16"
        {...strokeProps}
      />
    </>
  ),
  folder: <path d="M3.5 6.5h6l2 2h9v10h-17z" {...strokeProps} />,
  hand: (
    <path
      d="M8.5 12.5V6a1.5 1.5 0 0 1 3 0v5.5M11.5 11V4.8a1.5 1.5 0 0 1 3 0V11M14.5 11V6.3a1.5 1.5 0 0 1 3 0v7.2c0 4-2.4 6.5-6 6.5-2.6 0-4.1-1.3-5.4-3.5l-2-3.4a1.4 1.4 0 0 1 2.3-1.6l1.6 2"
      {...strokeProps}
    />
  ),
  home: <path d="M4.5 11 12 4.5l7.5 6.5M6.5 9.5v10h4v-5.5h3v5.5h4v-10" {...strokeProps} />,
  model: (
    <>
      <path d="m12 3.7 7.5 4.1v8.4L12 20.3l-7.5-4.1V7.8z" {...strokeProps} />
      <path d="m4.5 7.8 7.5 4.1 7.5-4.1M12 11.9v8.4" {...strokeProps} />
    </>
  ),
  more: (
    <>
      <circle cx="6" cy="12" r="1.2" fill="currentColor" />
      <circle cx="12" cy="12" r="1.2" fill="currentColor" />
      <circle cx="18" cy="12" r="1.2" fill="currentColor" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" {...strokeProps} />,
  review: (
    <>
      <rect height="15" rx="2" width="15" x="4.5" y="4.5" {...strokeProps} />
      <path d="m8 12 2.5 2.5L16 9" {...strokeProps} />
    </>
  ),
  refresh: (
    <>
      <path d="M19 8V4l-2 2a8 8 0 1 0 2.1 8" {...strokeProps} />
      <path d="M19 4h-4" {...strokeProps} />
    </>
  ),
  search: (
    <>
      <circle cx="10.7" cy="10.7" r="6.2" {...strokeProps} />
      <path d="m15.5 15.5 4 4" {...strokeProps} />
    </>
  ),
  send: <path d="m4 5 16 7-16 7 3-7zM7 12h13" {...strokeProps} />,
  settings: (
    <>
      <circle cx="12" cy="12" r="3" {...strokeProps} />
      <path
        d="M19.1 13.8c.1-.6.1-1.2 0-1.8l2-1.5-2-3.4-2.5 1a8 8 0 0 0-1.6-.9L14.6 4h-4L10 7.2c-.6.2-1.1.5-1.6.9l-2.5-1-2 3.4 2 1.5a8 8 0 0 0 0 1.8l-2 1.5 2 3.4 2.5-1c.5.4 1 .7 1.6.9l.5 3.2h4l.5-3.2c.6-.2 1.1-.5 1.6-.9l2.5 1 2-3.4z"
        {...strokeProps}
      />
    </>
  ),
  // A puzzle piece: Settings → 插件.
  puzzle: (
    <path
      d="M9 4.5h3.5a2 2 0 1 1 3.5 0H19v3.5a2 2 0 1 1 0 3.5V15h-3.5a2 2 0 1 0-3.5 0H8.5v-3.5a2 2 0 1 1 0-3.5V4.5Z"
      {...strokeProps}
    />
  ),
  // Settings navigation: Skill, 记忆, MCP, 命令 and 工具 each get their own mark.
  book: (
    <>
      <path d="M5.5 5.5A2.5 2.5 0 0 1 8 3h11v15H8a2.5 2.5 0 0 0-2.5 2.5z" {...strokeProps} />
      <path d="M5.5 20.5A2.5 2.5 0 0 1 8 18h11" {...strokeProps} />
    </>
  ),
  bookmark: <path d="M7 4.5h10v15l-5-3.5-5 3.5z" {...strokeProps} />,
  bell: (
    <path
      d="M6.5 16.5h11l-1.5-2.2V10.5a4 4 0 0 0-8 0v3.8zM10.3 19a1.8 1.8 0 0 0 3.4 0"
      {...strokeProps}
    />
  ),
  plug: <path d="M9 4v4M15 4v4M7 8h10v3.5a5 5 0 0 1-10 0zM12 16.5V20" {...strokeProps} />,
  slash: (
    <>
      <rect height="16" rx="3.5" width="16" x="4" y="4" {...strokeProps} />
      <path d="m14 8-4 8" {...strokeProps} />
    </>
  ),
  wrench: (
    <path
      d="M14.7 6.3a4 4 0 0 0-5.1 5.1l-5.1 5.1a1.5 1.5 0 0 0 0 2.1l.9.9a1.5 1.5 0 0 0 2.1 0l5.1-5.1a4 4 0 0 0 5.1-5.1l-2.4 2.4-2.1-.6-.6-2.1z"
      {...strokeProps}
    />
  ),
  shield: (
    <>
      <path d="M12 3.5 19 6v5.5c0 4.1-2.6 7.4-7 9-4.4-1.6-7-4.9-7-9V6z" {...strokeProps} />
      <path d="m8.5 12 2.2 2.2 4.8-5" {...strokeProps} />
    </>
  ),
  'sidebar-left': (
    <>
      <rect height="16" rx="2.5" width="18" x="3" y="4" {...strokeProps} />
      <path d="M8.5 4v16" {...strokeProps} />
    </>
  ),
  'sidebar-right': (
    <>
      <rect height="16" rx="2.5" width="18" x="3" y="4" {...strokeProps} />
      <path d="M15.5 4v16" {...strokeProps} />
    </>
  ),
  spark: (
    <>
      <path
        d="M12 3.5c.4 4.9 3.1 7.6 8 8-4.9.4-7.6 3.1-8 8-.4-4.9-3.1-7.6-8-8 4.9-.4 7.6-3.1 8-8Z"
        {...strokeProps}
      />
      <path d="M19 3.5v3M20.5 5h-3" {...strokeProps} />
    </>
  ),
  terminal: (
    <>
      <rect height="15" rx="2" width="17" x="3.5" y="4.5" {...strokeProps} />
      <path d="m7 9 3 3-3 3M12.5 15h4" {...strokeProps} />
    </>
  ),
  warning: (
    <>
      <path d="M12 4 21 20H3Z" {...strokeProps} />
      <path d="M12 9v5M12 17.2h.01" {...strokeProps} />
    </>
  ),
  workspace: (
    <>
      <path d="M4 5.5h6.5l1.5 2h8v11H4z" {...strokeProps} />
      <path d="M4 9.5h16" {...strokeProps} />
    </>
  ),
};
