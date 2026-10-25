import { useRef, type RefObject } from 'react';

import { ActivityNoteStack } from './ActivityNoteStack';
import { AppIcon } from './app-icons';

export function ConversationPreview({
  combinedSidebarWidthRatio,
  viewportRef,
}: {
  combinedSidebarWidthRatio: number;
  viewportRef: RefObject<HTMLElement | null>;
}) {
  const conversationColumnRef = useRef<HTMLDivElement>(null);

  return (
    <>
      <div className="conversation-column" ref={conversationColumnRef}>
        <article className="message message-user">
          <div className="message-avatar" aria-hidden="true">
            你
          </div>
          <div className="message-body">
            <div className="message-meta">
              <strong>你</strong>
              <time>10:24</time>
            </div>
            <div className="message-content">
              <p>
                按照详细设计实现 P05 模型配置、Transport 与三类 Protocol
                Adapter，并先完成可验收的前置界面。
              </p>
            </div>
          </div>
        </article>

        <article className="message message-aura">
          <div className="message-avatar aura-avatar" aria-hidden="true">
            <AppIcon name="spark" size={17} />
          </div>
          <div className="message-body">
            <div className="message-meta">
              <strong>Aura</strong>
              <time>10:25</time>
            </div>
            <div className="message-content">
              <p>
                我会保持 Renderer 与 Core 边界：Provider 目录、Endpoint、模型和凭据都只接受 Core
                Projection；当前交互只用于界面验收。
              </p>
            </div>

            <div className="tool-result-card">
              <div className="tool-result-heading">
                <span className="tool-result-icon">
                  <AppIcon name="check" size={15} />
                </span>
                <strong>已确认界面约束</strong>
                <span>演示</span>
              </div>
              <ul>
                <li>设置作为独立页面打开，不占用会话工作区</li>
                <li>模型按 Provider → Endpoint → Model Profile 管理</li>
                <li>右侧工具工作区和 Composer 高度均支持边界拖动</li>
              </ul>
            </div>
          </div>
        </article>
      </div>

      <ActivityNoteStack
        combinedSidebarWidthRatio={combinedSidebarWidthRatio}
        conversationColumnRef={conversationColumnRef}
        viewportRef={viewportRef}
      />
    </>
  );
}
