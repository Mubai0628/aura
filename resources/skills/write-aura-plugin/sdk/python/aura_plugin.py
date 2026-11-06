"""Aura 插件后台服务的帮助库，协议第 1 版。只用 Python 标准库（3.9 及以上）。

复制进插件文件夹（例如 service/aura_plugin.py），在服务里：

    from aura_plugin import AuraError, AuraService

    aura = AuraService()
    hello = aura.start([{"id": "qq-main", "max_text_chars": 2000, "groups": True}])
    aura.on_reply(send_to_platform)      # Aura 要发回聊天的话，一个 dict
    aura.on_commands(set_menu)           # 报了 command_menu 的渠道的整份命令菜单
    aura.on_approval(ask_in_chat)        # 交给私聊、等那个人决定的审批
    aura.on_shutdown(disconnect)         # Aura 让服务退出
    ...                                  # 连上平台，收到消息时：
    answer = aura.message_in("qq-main", "private", chat_id, user_id, message_id, text,
                             user_name="老王")
    aura.wait()                          # 等到 Aura 让服务退出

start() 之后 print() 写到标准错误（进插件日志），标准输出只留给协议。
处理函数在帮助库自己的线程里一个接一个地跑；可以是 async 函数：给了
use_loop(loop) 就交给那个事件循环，否则各自用 asyncio.run 跑完。
处理函数登记之前收到的 reply、commands、approval 先存着（每种最多 HELD_MAX 条，
多了丢最早的），登记时按收到的顺序交给它。
"""

import asyncio
import collections
import inspect
import json
import os
import queue
import sys
import threading

PROTOCOL = 1
MAX_LINE_BYTES = 1024 * 1024
MAX_IN_FLIGHT = 16
HELD_MAX = 100

__all__ = ["AuraError", "AuraService", "split_text"]


class AuraError(Exception):
    """Aura 拒绝了一个请求：code 是固定的代码，message 是给人看的原因。"""

    def __init__(self, code, message):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


def _stopping():
    return AuraError("aura.stopping", "Aura 已经让这个服务退出")


class _Waiter:
    def __init__(self):
        self.done = threading.Event()
        self.response = None


async def _awaited(value):
    return await value


class AuraService:
    def __init__(self, stdin=None, stdout=None):
        self._in = stdin if stdin is not None else sys.stdin.buffer
        self._out = stdout if stdout is not None else sys.stdout.buffer
        self._own_stdout = stdout is None
        self._write_lock = threading.Lock()
        self._pending = {}
        self._pending_lock = threading.Lock()
        self._closed = False
        self._slots = threading.Semaphore(MAX_IN_FLIGHT)
        self._next_id = 0
        self._handlers = {"reply": [], "commands": [], "approval": []}
        # Only the dispatch thread touches what is held.
        self._held = {kind: collections.deque(maxlen=HELD_MAX) for kind in self._handlers}
        self._shutdown_handlers = []
        self._events = queue.Queue()
        self._ended = threading.Event()
        self._loop = None
        self.hello = None

    # 启动与结束

    def start(self, channels=None):
        """读 Aura 的 hello，回 ready，开始收发。返回 hello（dict）。

        channels 每个渠道一项：id、max_text_chars，可选 markdown、buttons、command_menu、groups。
        """
        line = self._read_line()
        if line is None:
            raise EOFError("Aura 没有发 hello 就关了标准输入")
        hello = json.loads(line)
        if not isinstance(hello, dict) or hello.get("type") != "hello":
            raise RuntimeError(f"Aura 的第一行不是 hello：{line[:80]}")
        if hello.get("protocol") != PROTOCOL:
            raise RuntimeError(f"这个帮助库只会协议第 {PROTOCOL} 版，Aura 说的是 {hello.get('protocol')}")
        self.hello = hello
        if self._own_stdout:
            sys.stdout = sys.stderr
        self._send({"type": "ready", "protocol": PROTOCOL, "channels": list(channels or [])})
        threading.Thread(target=self._read_loop, name="aura-read", daemon=True).start()
        threading.Thread(target=self._dispatch_loop, name="aura-dispatch", daemon=True).start()
        return hello

    def wait(self, timeout=None):
        """等到 Aura 让服务退出（或关了标准输入），退出的处理函数都跑完。到了返回 True。"""
        return self._ended.wait(timeout)

    @property
    def ended(self):
        return self._ended.is_set()

    def use_loop(self, loop):
        """async 的处理函数交给这个事件循环跑。"""
        self._loop = loop

    def on_reply(self, handler):
        """handler(reply)：reply 有 channel、chat（kind、id）、reply_to（可能没有）、text、format；
        Aura 按渠道的上限分了段时还有 part、parts。"""
        return self._on("reply", handler)

    def on_commands(self, handler):
        """handler(menu)：menu 有 channel、commands（每项 name、title、description，有参数的还有
        argument_hint）。只发给 ready 里报了 command_menu 的渠道，每次是整份菜单。"""
        return self._on("commands", handler)

    def on_approval(self, handler):
        """handler(asked)：asked 有 channel、chat（kind、id）、approval（id、title、detail、
        decisions）、text。插件打开了远程批准时，交给私聊、等那个人决定的审批：有按钮的平台按
        decisions 画按钮，没有的把 text 发出去。用户的回复或按下的按钮当一条消息交给 message_in。"""
        return self._on("approval", handler)

    def _on(self, kind, handler):
        self._handlers[kind].append(handler)
        # Tells the dispatch thread to hand over what it held for this kind, in order.
        self._events.put((kind, None))
        return handler

    def on_shutdown(self, handler):
        """handler()：Aura 让服务退出，或关了标准输入。都跑完后 wait() 返回。"""
        self._shutdown_handlers.append(handler)
        return handler

    # 设置

    @property
    def plugin(self):
        return (self.hello or {}).get("plugin")

    @property
    def channels(self):
        """清单里的渠道：id、platform、title。"""
        return list((self.hello or {}).get("channels", []))

    def setting(self, key, default=None):
        """一个文字设置在 hello 时的值。"""
        return (self.hello or {}).get("settings", {}).get(key, default)

    def secret(self, key):
        """一个保密设置的值，没填时是 None。不要写进日志或文件。"""
        return os.environ.get("AURA_SECRET_" + key.upper())

    @property
    def data_folder(self):
        """数据只写在这里，不要写进插件文件夹。"""
        return os.environ.get("AURA_PLUGIN_DATA", os.getcwd())

    # 请求

    def call(self, method, params=None, timeout=None):
        """调 Aura 的一个方法，等它回答。被拒时抛 AuraError；过了 timeout 秒抛 TimeoutError。

        同时最多 16 个请求在等回答，多的在这里等空位；超时的请求等到回答才让出空位。
        """
        self._slots.acquire()
        waiter = _Waiter()
        with self._pending_lock:
            if self._closed:
                self._slots.release()
                raise _stopping()
            self._next_id += 1
            request_id = self._next_id
            self._pending[request_id] = waiter
        request = {"type": "request", "id": request_id, "method": method, "params": params or {}}
        try:
            self._send(request)
        except (OSError, ValueError):
            with self._pending_lock:
                self._pending.pop(request_id, None)
            self._slots.release()
            raise
        if not waiter.done.wait(timeout):
            raise TimeoutError(f"{method} 没有在 {timeout} 秒内得到回答")
        response = waiter.response
        if response is None:
            raise _stopping()
        error = response.get("error")
        if error:
            raise AuraError(error.get("code", "aura.failed"), error.get("message", ""))
        return response.get("result")

    async def acall(self, method, params=None, timeout=None):
        """call 的 async 版，在别的线程里等，不卡住事件循环。"""
        return await asyncio.to_thread(self.call, method, params, timeout)

    def message_in(self, channel, chat_kind, chat_id, user_id, message_id, text,
                   user_name=None, reply_to=None):
        """把平台推来的一条用户消息交给 Aura。chat_kind 是 "private" 或 "group"。"""
        params = {
            "channel": channel,
            "chat": {"kind": chat_kind, "id": str(chat_id)},
            "user": {"id": str(user_id)},
            "message_id": str(message_id),
            "text": text,
        }
        if user_name:
            params["user"]["name"] = user_name
        if reply_to is not None:
            params["reply_to"] = str(reply_to)
        return self.call("message.in", params)

    async def amessage_in(self, *args, **kwargs):
        return await asyncio.to_thread(self.message_in, *args, **kwargs)

    def notify(self, text, tone="info"):
        return self.call("notify", {"text": text, "tone": tone})

    def status(self, text=None, tone="info"):
        """状态栏的一条；text 为 None 清掉。"""
        params = {"tone": tone}
        if text is not None:
            params["text"] = text
        return self.call("status", params)

    def settings(self):
        """文字设置现在的值。"""
        return self.call("settings.get")["values"]

    def set_setting(self, key, value):
        """改一个文字设置，空字符串清掉，返回改后的文字设置。之后服务会带着新值重启。"""
        return self.call("settings.set", {"key": key, "value": value})["values"]

    def sessions(self):
        return self.call("sessions.list")

    def session_state(self, session_id):
        return self.call("session.state", {"session_id": session_id})

    def session_events(self, session_id, after=None):
        params = {"session_id": session_id}
        if after is not None:
            params["after"] = after
        return self.call("session.events", params)

    def send(self, session_id, text, dedupe_key=None):
        params = {"session_id": session_id, "text": text}
        if dedupe_key is not None:
            params["dedupe_key"] = dedupe_key
        return self.call("session.send", params)

    def run_command(self, session_id, name, arguments="", dedupe_key=None):
        """在会话里运行插件自己的一个提示命令（name 不带 /），作为插件的消息发出。"""
        params = {"session_id": session_id, "name": name}
        if arguments:
            params["arguments"] = arguments
        if dedupe_key is not None:
            params["dedupe_key"] = dedupe_key
        return self.call("command.run", params)

    def stop(self, session_id):
        return self.call("session.stop", {"session_id": session_id})

    def new_session(self, workspace_id=None, title=None, dedupe_key=None):
        params = {}
        for key, value in (("workspace_id", workspace_id), ("title", title), ("dedupe_key", dedupe_key)):
            if value is not None:
                params[key] = value
        return self.call("session.new", params)

    def log(self, text, level="info"):
        """写进插件日志：level 是 info、warning 或 error。Aura 不在时什么也不做。"""
        try:
            self._send({"type": "log", "level": level, "text": str(text)[:4000]})
        except (OSError, ValueError):
            pass

    # 收发

    def _send(self, message):
        data = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        if len(data) > MAX_LINE_BYTES:
            raise ValueError("一行最多 1 MiB")
        with self._write_lock:
            self._out.write(data + b"\n")
            self._out.flush()

    def _read_line(self):
        while True:
            raw = self._in.readline(MAX_LINE_BYTES + 2)
            if not raw:
                return None
            if not raw.endswith(b"\n") and len(raw) > MAX_LINE_BYTES:
                while raw and not raw.endswith(b"\n"):
                    raw = self._in.readline(MAX_LINE_BYTES + 2)
                continue
            return raw.decode("utf-8", errors="replace").rstrip("\r\n")

    def _read_loop(self):
        try:
            while True:
                line = self._read_line()
                if line is None:
                    break
                try:
                    message = json.loads(line)
                except ValueError:
                    continue
                kind = message.get("type") if isinstance(message, dict) else None
                if kind == "response":
                    self._answered(message)
                elif kind in ("reply", "commands", "approval", "error"):
                    self._events.put((kind, message))
                elif kind == "shutdown":
                    break
        except OSError:
            pass
        finally:
            with self._pending_lock:
                self._closed = True
                waiting, self._pending = self._pending, {}
            for waiter in waiting.values():
                waiter.done.set()
                self._slots.release()
            self._events.put(("shutdown", None))

    def _answered(self, response):
        with self._pending_lock:
            waiter = self._pending.pop(response.get("id"), None)
        if waiter is None:
            return
        waiter.response = response
        waiter.done.set()
        self._slots.release()

    def _dispatch_loop(self):
        while True:
            kind, message = self._events.get()
            if kind in self._handlers:
                held = self._held[kind]
                if message is not None:
                    held.append(message)
                handlers = list(self._handlers[kind])
                while handlers and held:
                    waiting = held.popleft()
                    for handler in handlers:
                        self._run(handler, waiting)
            elif kind == "error":
                print(f"Aura：{message.get('code')}：{message.get('message')}", file=sys.stderr)
            else:
                for handler in list(self._shutdown_handlers):
                    self._run(handler)
                self._ended.set()
                return

    def _run(self, handler, *args):
        try:
            outcome = handler(*args)
            if inspect.isawaitable(outcome):
                if self._loop is not None and self._loop.is_running():
                    asyncio.run_coroutine_threadsafe(_awaited(outcome), self._loop).result()
                else:
                    asyncio.run(_awaited(outcome))
        except Exception as error:  # noqa: BLE001 - a handler's failure is the plugin's to read
            self.log(f"处理 Aura 发来的消息时出错：{error!r}", "error")


def split_text(text, max_chars):
    """把一段话按平台一条消息的上限分成几段：先在空行处分，再在换行处分，最后硬切。"""
    if max_chars < 1:
        raise ValueError("max_chars 至少是 1")
    pieces = []
    rest = text
    while len(rest) > max_chars:
        window = rest[:max_chars]
        cut = window.rfind("\n\n")
        if cut <= 0:
            cut = window.rfind("\n")
        if cut <= 0:
            cut = max_chars
        piece = rest[:cut].rstrip("\n")
        if piece:
            pieces.append(piece)
        rest = rest[cut:].lstrip("\n")
    if rest or not pieces:
        pieces.append(rest)
    return pieces
