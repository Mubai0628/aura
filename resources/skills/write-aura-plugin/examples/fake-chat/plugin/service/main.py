"""假聊天平台的渠道适配：长轮询收消息交给 Aura，把 Aura 的回复和交来的审批发回去。"""

import json
import threading
import time
import urllib.error
import urllib.request

from aura_plugin import AuraError, AuraService

CHANNEL = "fake-main"
# 平台一条消息最多几个字符：Aura 按它把回复分好段。
MAX_TEXT_CHARS = 500

aura = AuraService()
aura.start([{"id": CHANNEL, "max_text_chars": MAX_TEXT_CHARS, "groups": True}])
address = aura.setting("address", "").rstrip("/")
# 假平台在本机：不经过系统代理。真平台照常用用户的代理设置。
local = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def platform(method, path, body=None, timeout=35):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
    request = urllib.request.Request(address + path, data=data, method=method)
    request.add_header("Content-Type", "application/json; charset=utf-8")
    with local.open(request, timeout=timeout) as response:
        return json.loads(response.read() or b"{}")


@aura.on_reply
def reply(message):
    chat = {"type": message["chat"]["kind"], "id": message["chat"]["id"]}
    try:
        platform("POST", "/send", {"chat": chat, "reply_to": message.get("reply_to"), "text": message["text"]})
    except (OSError, urllib.error.URLError) as error:
        aura.log(f"回复没有发出去：{error}", "warning")


@aura.on_approval
def approval(asked):
    # 假平台没有按钮：整段文字发到那个私聊。那个人回的 /approve 之类照常是一条消息，由 receive 交给 Aura。
    chat = {"type": asked["chat"]["kind"], "id": asked["chat"]["id"]}
    try:
        platform("POST", "/send", {"chat": chat, "text": asked["text"]})
    except (OSError, urllib.error.URLError) as error:
        aura.log(f"审批没有发出去：{error}", "warning")


def receive():
    offset, delay = 1, 1
    while not aura.ended:
        try:
            updates = platform("GET", f"/updates?offset={offset}&timeout=25")["updates"]
            delay = 1
        except (OSError, urllib.error.URLError, ValueError) as error:
            aura.log(f"连不上假平台 {address}：{error}；{delay} 秒后再试", "warning")
            time.sleep(delay)
            delay = min(delay * 2, 60)
            continue
        for update in updates:
            offset = max(offset, update["update_id"] + 1)
            message = update["message"]
            try:
                answer = aura.message_in(
                    CHANNEL,
                    message["chat"]["type"],
                    message["chat"]["id"],
                    message["from"]["id"],
                    message["message_id"],
                    message["text"],
                    user_name=message["from"].get("name"),
                )
                aura.log(f"消息 {message['message_id']}：{json.dumps(answer, ensure_ascii=False)}")
            except AuraError as error:
                aura.log(f"Aura 没有收下消息 {message['message_id']}：{error.code} {error.message}", "warning")


if not address:
    aura.log("没有填假平台的地址", "error")
else:
    threading.Thread(target=receive, name="receive", daemon=True).start()
aura.wait()
