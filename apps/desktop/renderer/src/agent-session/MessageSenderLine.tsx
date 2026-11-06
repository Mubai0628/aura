import type { MessageSender } from '@aura/contracts/generated';
import { PLATFORM_LABELS } from '../plugin-settings/plugin-labels';
import './message-sender.css';

/** Who sent a message the user did not type: the plugin, and who wrote it on its chat platform. */
export function MessageSenderLine({ sender }: { sender: MessageSender | null | undefined }) {
  if (!sender) return null;
  const from = sender.from;
  if (!from) return <p className="message-sender-line">来自插件 {sender.plugin}</p>;
  const platform = (PLATFORM_LABELS as Record<string, string>)[from.platform] ?? from.platform;
  const where = from.chat === 'group' ? `${platform} 群聊` : platform;
  return (
    <p className="message-sender-line" title={`${from.channel} · ${from.user_id}`}>
      来自插件 {sender.plugin} · {where} · {from.user_name || from.user_id}
    </p>
  );
}
