#!/usr/bin/env python3
import json
import subprocess
import sys
import threading


class JsonRpcClient:
    def __init__(self, command: list[str]) -> None:
        self.next_id = 1
        self.lock = threading.Lock()
        self.pending: dict[int, tuple[threading.Event, dict]] = {}
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,
            text=True,
            bufsize=1,
        )
        self.reader = threading.Thread(target=self._read_loop, daemon=True)
        self.reader.start()

    def request(
        self,
        method: str,
        params: dict | None = None,
        timeout_s: float = 60.0,
    ) -> dict:
        with self.lock:
            request_id = self.next_id
            self.next_id += 1
        event = threading.Event()
        slot: dict = {}
        with self.lock:
            self.pending[request_id] = (event, slot)
        payload = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params or {},
        }
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(payload) + "\n")
        self.process.stdin.flush()
        if not event.wait(timeout_s):
            with self.lock:
                self.pending.pop(request_id, None)
            raise TimeoutError(f"json-rpc request timed out: {method}")
        if "error" in slot:
            raise RuntimeError(slot["error"]["message"])
        return slot["result"]

    def on_notification(self, message: dict) -> None:
        if message.get("method") != "display/event":
            return
        event = message["params"]["event"]
        if event["type"] == "assistant_message_delta":
            print(event["text"], end="", flush=True)
        elif event["type"] == "assistant_message_final":
            print()
        elif event["type"] == "approval_requested":
            approval_id = event["approval"]["approvalId"]
            print(f"approval requested: {approval_id}", file=sys.stderr)

    def _read_loop(self) -> None:
        assert self.process.stdout is not None
        try:
            for line in self.process.stdout:
                message = json.loads(line)
                if "method" in message:
                    self.on_notification(message)
                    continue
                request_id = message["id"]
                with self.lock:
                    pending = self.pending.pop(request_id, None)
                if pending is None:
                    continue
                event, slot = pending
                if "error" in message:
                    slot["error"] = message["error"]
                else:
                    slot["result"] = message["result"]
                event.set()
        except Exception as error:
            self._fail_pending(str(error))
            return
        self._fail_pending("json-rpc server closed stdout")

    def _fail_pending(self, message: str) -> None:
        with self.lock:
            pending = list(self.pending.values())
            self.pending.clear()
        for event, slot in pending:
            slot["error"] = {"message": message}
            event.set()


def main() -> None:
    command = sys.argv[1:] or ["noloong-agent-interaction-server"]
    client = JsonRpcClient(command)
    client.request(
        "initialize",
        {
            "name": "python-example-bridge",
            "requestedAuthority": ["agent.run", "approval.resolve"],
            "requestedUx": {
                "displayEvents": True,
                "streamText": False,
                "markdown": True,
                "maxMessageBytes": 8192,
            },
        },
    )
    client.request("session/create", {"sessionId": "example", "profileId": "default"})
    client.request(
        "display/subscribe",
        {
            "sessionId": "example",
            "ux": {
                "displayEvents": True,
                "streamText": False,
                "maxMessageBytes": 8192,
            },
        },
    )
    client.request(
        "agent/prompt",
        {
            "sessionId": "example",
            "input": {
                "type": "text",
                "text": "Say hello from the Python bridge.",
            },
        },
    )
    client.request("shutdown", {})


if __name__ == "__main__":
    main()
