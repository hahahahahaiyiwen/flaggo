import { createServer, type ServerResponse } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { diag, DiagConsoleLogger, DiagLogLevel } from "@opentelemetry/api";

import {
  loadPlaygroundConfiguration,
  TetrisPlayground,
} from "./playground.js";
import { PlaygroundRequestError } from "./errors.js";
import type { TetrisSessionCommand } from "./types.js";

const sourceDirectory = dirname(fileURLToPath(import.meta.url));
const publicDirectory = resolve(sourceDirectory, "../public");
diag.setLogger(new DiagConsoleLogger(), DiagLogLevel.ERROR);
const configuration = loadPlaygroundConfiguration();
const playground = new TetrisPlayground(configuration);
const clients = new Set<ServerResponse>();
const commands = new Set<TetrisSessionCommand>([
  "move-left",
  "move-right",
  "rotate-clockwise",
  "soft-drop",
  "hard-drop",
  "gravity-tick",
  "pause",
  "resume",
  "restart",
]);

const address = parseAddress(
  process.env.FLAGGO_PLAYGROUND_ADDRESS ?? "127.0.0.1:4310",
);

const unsubscribe = playground.subscribe((snapshot) => {
  const payload = `event: state\ndata: ${JSON.stringify(snapshot)}\n\n`;
  for (const client of clients) client.write(payload);
});

const server = createServer(async (request, response) => {
  try {
    const url = new URL(
      request.url ?? "/",
      `http://${request.headers.host ?? "127.0.0.1"}`,
    );
    if (request.method === "GET" && url.pathname === "/api/health") {
      json(response, 200, { status: "ready" });
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/state") {
      json(response, 200, playground.snapshot());
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/shutdown") {
      json(response, 202, { status: "stopping" });
      setImmediate(() => void close(0));
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/events") {
      response.writeHead(200, {
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
        "Content-Type": "text/event-stream",
        "X-Accel-Buffering": "no",
      });
      clients.add(response);
      response.write(
        `event: state\ndata: ${JSON.stringify(playground.snapshot())}\n\n`,
      );
      request.on("close", () => clients.delete(response));
      return;
    }
    if (
      request.method === "POST"
      && url.pathname.startsWith("/api/commands/")
    ) {
      const command = url.pathname.slice("/api/commands/".length);
      if (!commands.has(command as TetrisSessionCommand)) {
        json(response, 400, { error: `Unknown Tetris command '${command}'.` });
        return;
      }
      json(
        response,
        200,
        playground.command(command as TetrisSessionCommand),
      );
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/demo/run") {
      json(response, 202, playground.runDemo());
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/demo/cancel") {
      json(response, 202, playground.cancelDemo());
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/bots/start") {
      json(response, 202, playground.startAllBots());
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/bots/stop") {
      json(response, 200, await playground.stopAllBots());
      return;
    }
    if (request.method === "POST" && url.pathname.startsWith("/api/bots/")) {
      const parts = url.pathname.split("/");
      const id = parts[3];
      const action = parts[4];
      if (parts.length === 5 && id !== undefined && action === "start") {
        json(response, 202, playground.startBot(id));
        return;
      }
      if (parts.length === 5 && id !== undefined && action === "stop") {
        json(response, 200, await playground.stopBot(id));
        return;
      }
    }
    if (
      request.method === "POST"
      && url.pathname === "/api/executables/history"
    ) {
      json(response, 200, await playground.showExecutableHistory(
        url.searchParams.get("cursor") ?? undefined,
      ));
      return;
    }
    if (
      request.method === "POST"
      && url.pathname.startsWith("/api/executables/")
      && url.pathname.endsWith("/activate")
    ) {
      const encodedDigest = url.pathname.slice(
        "/api/executables/".length,
        -"/activate".length,
      );
      const executableDigest = decodeURIComponent(encodedDigest);
      json(
        response,
        200,
        await playground.activateExecutable(executableDigest),
      );
      return;
    }
    if (request.method === "GET" && url.pathname === "/") {
      await file(response, resolve(publicDirectory, "index.html"), "text/html");
      return;
    }
    if (request.method === "GET" && url.pathname === "/app.js") {
      await file(
        response,
        resolve(sourceDirectory, "client.js"),
        "text/javascript",
      );
      return;
    }
    if (request.method === "GET" && url.pathname === "/styles.css") {
      await file(
        response,
        resolve(publicDirectory, "styles.css"),
        "text/css",
      );
      return;
    }
    json(response, 404, { error: "Route not found." });
  } catch (error) {
    json(
      response,
      error instanceof PlaygroundRequestError ? error.status : 500,
      { error: errorMessage(error) },
    );
  }
});

const keepAlive = setInterval(() => {
  for (const client of clients) client.write(": keepalive\n\n");
}, 15_000);

let closing = false;
async function close(exitCode: number): Promise<void> {
  if (closing) return;
  closing = true;
  clearInterval(keepAlive);
  unsubscribe();
  for (const client of clients) client.end();
  clients.clear();
  await new Promise<void>((resolveClose, rejectClose) => {
    server.close((error) => {
      if (error === undefined) resolveClose();
      else rejectClose(error);
    });
  });
  await playground.close();
  process.exitCode = exitCode;
}

process.once("SIGINT", () => void close(130));
process.once("SIGTERM", () => void close(143));

server.listen(address.port, address.host, () => {
  playground.start();
  process.stdout.write(
    `FLAGGO_PLAYGROUND_READY ${JSON.stringify({
      url: `http://${address.host}:${address.port}`,
    })}\n`,
  );
});

function parseAddress(value: string): { readonly host: string; readonly port: number } {
  const separator = value.lastIndexOf(":");
  const host = value.slice(0, separator);
  const port = Number(value.slice(separator + 1));
  if (
    separator <= 0
    || host === ""
    || !Number.isInteger(port)
    || port < 1
    || port > 65_535
  ) {
    throw new Error(
      "FLAGGO_PLAYGROUND_ADDRESS must use the form '<host>:<port>'.",
    );
  }
  return { host, port };
}

async function file(
  response: ServerResponse,
  path: string,
  contentType: string,
): Promise<void> {
  const content = await readFile(path);
  response.writeHead(200, {
    "Cache-Control": "no-store",
    "Content-Type": `${contentType}; charset=utf-8`,
  });
  response.end(content);
}

function json(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Type": "application/json; charset=utf-8",
  });
  response.end(JSON.stringify(value));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
