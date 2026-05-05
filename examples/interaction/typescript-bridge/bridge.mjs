#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

class JsonRpcClient {
  constructor(command, args = []) {
    this.nextId = 1;
    this.pending = new Map();
    this.child = spawn(command, args, {
      stdio: ["pipe", "pipe", "inherit"],
    });
    const lines = createInterface({ input: this.child.stdout });
    lines.on("line", (line) => this.#handleLine(line));
  }

  request(method, params = {}) {
    const id = this.nextId++;
    const payload = { jsonrpc: "2.0", id, method, params };
    const promise = new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
    this.child.stdin.write(`${JSON.stringify(payload)}\n`);
    return promise;
  }

  onNotification(message) {
    if (message.method === "display/event") {
      const event = message.params.event;
      if (event.type === "assistant_message_delta") {
        process.stdout.write(event.text);
      } else if (event.type === "assistant_message_final") {
        process.stdout.write("\n");
      } else if (event.type === "approval_requested") {
        console.error(`approval requested: ${event.approval.approvalId}`);
      }
    }
  }

  #handleLine(line) {
    const message = JSON.parse(line);
    if (message.method) {
      this.onNotification(message);
      return;
    }
    const pending = this.pending.get(message.id);
    if (!pending) {
      return;
    }
    this.pending.delete(message.id);
    if (message.error) {
      pending.reject(new Error(message.error.message));
    } else {
      pending.resolve(message.result);
    }
  }
}

async function main() {
  const command = process.argv[2] ?? "noloong-agent-interaction-server";
  const client = new JsonRpcClient(command, process.argv.slice(3));
  await client.request("initialize", {
    name: "typescript-example-bridge",
    requestedAuthority: ["agent.run", "approval.resolve"],
    requestedUx: {
      displayEvents: true,
      streamText: true,
      editMessage: true,
      markdown: true,
      maxMessageBytes: 8192,
    },
  });
  await client.request("session/create", {
    sessionId: "example",
    profileId: "default",
  });
  await client.request("display/subscribe", {
    sessionId: "example",
    ux: {
      displayEvents: true,
      streamText: true,
      editMessage: true,
      maxMessageBytes: 8192,
    },
  });
  await client.request("agent/prompt", {
    sessionId: "example",
    input: {
      type: "text",
      text: "Say hello from the TypeScript bridge.",
    },
  });
  await client.request("shutdown", {});
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
