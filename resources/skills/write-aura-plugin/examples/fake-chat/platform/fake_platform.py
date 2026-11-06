"""本机的假聊天平台，用来试渠道适配。只用 Python 标准库。

    python fake_platform.py --port 8765

接口（都是 JSON）：
- GET  /updates?offset=N&timeout=S  长轮询：编号大于等于 N 的新消息，没有就最多等 S 秒（默认 25）。
- POST /send                         机器人发消息：{"chat": {"type", "id"}, "reply_to", "text"}。
- POST /push                         测试的人替用户发一条：{"chat_type", "chat_id", "user_id",
                                     "user_name", "text", "message_id"（可省，省了自动编）}。
- GET  /sent                         机器人发出的全部消息。
"""

import argparse
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

updates = []
sent = []
changed = threading.Condition()


class Platform(BaseHTTPRequestHandler):
    def _json(self, status, value):
        body = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        length = int(self.headers.get("Content-Length", "0"))
        return json.loads(self.rfile.read(length) or b"{}")

    def do_GET(self):  # noqa: N802 - the name http.server calls
        url = urlparse(self.path)
        query = parse_qs(url.query)
        if url.path == "/updates":
            offset = int(query.get("offset", ["1"])[0])
            timeout = float(query.get("timeout", ["25"])[0])
            with changed:
                changed.wait_for(lambda: any(u["update_id"] >= offset for u in updates), timeout)
                fresh = [u for u in updates if u["update_id"] >= offset]
            self._json(200, {"updates": fresh})
        elif url.path == "/sent":
            with changed:
                self._json(200, {"sent": list(sent)})
        else:
            self._json(404, {"error": "not found"})

    def do_POST(self):  # noqa: N802 - the name http.server calls
        url = urlparse(self.path)
        body = self._body()
        with changed:
            if url.path == "/push":
                update_id = len(updates) + 1
                message = {
                    "message_id": body.get("message_id") or f"m-{update_id}",
                    "chat": {"type": body.get("chat_type", "private"), "id": body["chat_id"]},
                    "from": {"id": body["user_id"], "name": body.get("user_name", "")},
                    "text": body["text"],
                }
                updates.append({"update_id": update_id, "message": message})
                changed.notify_all()
                self._json(200, {"update_id": update_id, "message_id": message["message_id"]})
            elif url.path == "/send":
                sent.append(body)
                changed.notify_all()
                self._json(200, {"ok": True})
            else:
                self._json(404, {"error": "not found"})

    def log_message(self, format, *args):  # noqa: A002 - the name http.server calls
        pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8765)
    port = parser.parse_args().port
    server = ThreadingHTTPServer(("127.0.0.1", port), Platform)
    print(f"假聊天平台在 http://127.0.0.1:{server.server_address[1]}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
