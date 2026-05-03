import process from "node:process";
import readline from "node:readline";

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };
export type JsonRpcId = string | number | null;

const JSON_RPC_METHODS = [
  "initialize",
  "capabilities/list",
  "model/stream",
  "tool/execute",
  "context/apply",
  "phase/run",
  "phase_hook/run",
  "tool_hook/run",
  "compaction/summarize",
  "shutdown",
] as const;

const JSON_RPC_METHOD_SET: ReadonlySet<string> = new Set(JSON_RPC_METHODS);

export type JsonRpcMethod = (typeof JSON_RPC_METHODS)[number];

export type ExtensionContext = {
  streamEvent(streamId: string, event: JsonValue): void;
  log(message: string): void;
};

export type ExtensionHandler = (
  params: JsonObject,
  context: ExtensionContext,
) => unknown | Promise<unknown>;

export type ExtensionHandlers = Partial<Record<JsonRpcMethod, ExtensionHandler>>;

export function createStdioJsonRpcExtension(handlers: ExtensionHandlers) {
  return {
    async serve(): Promise<void> {
      const rl = readline.createInterface({
        input: process.stdin,
        crlfDelay: Infinity,
      });

      const context: ExtensionContext = {
        streamEvent(streamId, event) {
          send({
            jsonrpc: "2.0",
            method: "stream/event",
            params: { streamId, event },
          });
        },
        log(message) {
          process.stderr.write(`${message}\n`);
        },
      };

      for await (const line of rl) {
        if (!line.trim()) {
          continue;
        }
        const shouldStop = await handleLine(line, handlers, context);
        if (shouldStop) {
          rl.close();
          break;
        }
      }
    },
  };
}

async function handleLine(
  line: string,
  handlers: ExtensionHandlers,
  context: ExtensionContext,
): Promise<boolean> {
  let request: unknown;
  try {
    request = JSON.parse(line);
  } catch {
    error(null, -32700, "parse error");
    return false;
  }

  if (!isRecord(request)) {
    error(null, -32600, "invalid request");
    return false;
  }

  const id = readId(request.id);
  if (request.jsonrpc !== "2.0" || typeof request.method !== "string") {
    error(id, -32600, "invalid request");
    return false;
  }

  const handler = isJsonRpcMethod(request.method) ? handlers[request.method] : undefined;
  if (!handler) {
    error(id, -32601, `unknown method: ${request.method}`);
    return false;
  }

  try {
    const value = await handler(toJsonObject(request.params), context);
    result(id, value);
    return request.method === "shutdown";
  } catch (err) {
    context.log(err instanceof Error ? err.message : String(err));
    error(id, -32000, "handler failed");
    return false;
  }
}

function send(message: unknown) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function result(id: JsonRpcId, value: unknown) {
  send({
    jsonrpc: "2.0",
    id,
    result: value === undefined ? {} : value,
  });
}

function error(id: JsonRpcId, code: number, message: string) {
  send({
    jsonrpc: "2.0",
    id,
    error: { code, message },
  });
}

function readId(value: unknown): JsonRpcId {
  if (typeof value === "string" || typeof value === "number" || value === null) {
    return value;
  }
  return null;
}

function toJsonObject(value: unknown): JsonObject {
  return isRecord(value) ? (value as JsonObject) : {};
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isJsonRpcMethod(method: string): method is JsonRpcMethod {
  return JSON_RPC_METHOD_SET.has(method);
}
