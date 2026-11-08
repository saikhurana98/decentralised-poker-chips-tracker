import test from "node:test";
import assert from "node:assert/strict";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { WebSocket } from "ws";
import { z } from "zod";
import {
  credentialSchema,
  serverMessageSchema,
  type Credential,
  type State,
  type Command,
} from "../web/protocol.ts";
const origin = "http://localhost:18081";
const directory = mkdtempSync(join(tmpdir(), "poker-test-"));
let child: ChildProcess | undefined;
type Message = z.infer<typeof serverMessageSchema>;
class Client {
  readonly socket: WebSocket;
  readonly messages: Message[] = [];
  private waiters: {
    test: (m: Message) => boolean;
    resolve: (m: Message) => void;
  }[] = [];
  private latest: State | undefined;
  constructor() {
    this.socket = new WebSocket(origin.replace("http:", "ws:") + "/ws", {
      origin,
    });
    this.socket.on("message", (raw) => {
      const message = serverMessageSchema.parse(
        JSON.parse(
          (raw instanceof ArrayBuffer
            ? Buffer.from(raw)
            : Array.isArray(raw)
              ? Buffer.concat(raw)
              : raw
          ).toString(),
        ),
      );
      this.messages.push(message);
      if (message.type === "state") this.latest = message.state;
      for (const waiter of [...this.waiters])
        if (waiter.test(message)) {
          this.waiters = this.waiters.filter((w) => w !== waiter);
          waiter.resolve(message);
        }
    });
  }
  get state(): State {
    if (!this.latest) throw Error("No snapshot received");
    return this.latest;
  }
  async opened(): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      this.socket.once("open", resolve);
      this.socket.once("error", reject);
    });
  }
  send(value: unknown): void {
    this.socket.send(JSON.stringify(value));
  }
  async wait(predicate: (m: Message) => boolean): Promise<Message> {
    const existing = this.messages.find(predicate);
    if (existing) return existing;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(Error("Timed out waiting for message"));
      }, 5000);
      this.waiters.push({
        test: predicate,
        resolve: (m) => {
          clearTimeout(timer);
          resolve(m);
        },
      });
    });
  }
}
const clients: Client[] = [];
async function launch(): Promise<void> {
  child = spawn(process.env["RUST_SERVER"] ?? "./target/debug/poker-bank", [], {
    env: {
      ...process.env,
      PORT: "18081",
      PUBLIC_ORIGIN: origin,
      DB_PATH: join(directory, "test.db"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const running = child;
  let output = "";
  running.stderr?.on("data", (data: Buffer) => {
    output += data.toString();
  });
  for (let i = 0; i < 100; i++) {
    if (running.exitCode !== null)
      throw Error(`Server exited ${String(running.exitCode)}: ${output}`);
    try {
      const response = await fetch(origin + "/healthz");
      if (response.ok) return;
    } catch {
      /* Server is still starting. */
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw Error("Server startup timeout: " + output);
}
async function stop(): Promise<void> {
  const running = child;
  if (!running || running.exitCode !== null) return;
  await new Promise<void>((resolve) => {
    running.once("exit", () => {
      resolve();
    });
    running.kill("SIGTERM");
  });
}
async function api(
  path: string,
  body: unknown,
  from = origin,
): Promise<Response> {
  return fetch(origin + "/api/" + path, {
    method: "POST",
    headers: { origin: from, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}
async function seat(path: string, body: unknown): Promise<Credential> {
  const response = await api(path, body);
  assert.equal(response.status, 200);
  return credentialSchema.parse(await response.json());
}
async function connect(seat: Credential): Promise<Client> {
  const c = new Client();
  clients.push(c);
  await c.opened();
  c.send({ type: "auth", ...seat });
  await c.wait((m) => m.type === "state");
  return c;
}
async function cmd(
  client: Client,
  command: Command,
  options: { id?: string; revision?: number } = {},
): Promise<Message> {
  const id = options.id ?? randomUUID();
  client.send({
    type: "command",
    id,
    revision: options.revision ?? client.state.revision,
    command,
  });
  const result = await client.wait((m) => "id" in m && m.id === id);
  if (result.type === "ack")
    await client.wait(
      (m) => m.type === "state" && m.state.revision >= result.revision,
    );
  return result;
}
await test("WebSocket authority, persistence, lobby and restart", async (t) => {
  try {
    await launch();
    await t.test("origin and input validation", async () => {
      assert.equal(
        (await api("create", { name: "A" }, "https://evil.example")).status,
        403,
      );
      assert.equal((await api("create", { name: " " })).status, 400);
      assert.equal(
        (await api("create", { name: "A", minorPerChip: 1.5 })).status,
        400,
      );
    });
    const host = await seat("create", {
      name: "Host",
      title: "Test table",
      minorPerChip: 50,
      currency: "INR",
      defaultBuyin: 200,
    });
    const guest = await seat("join", { name: "Guest", code: host.code });
    const a = await connect(host),
      b = await connect(guest);
    await t.test("host approval and authorization", async () => {
      assert.equal(a.state.players.length, 2);
      assert.ok(!JSON.stringify(a.state).includes(host.token));
      assert.equal(a.state.minorPerChip, 50);
      assert.equal((await cmd(b, { type: "pause" })).type, "error");
      assert.equal(
        (
          await cmd(a, {
            type: "approve",
            playerId: host.playerId,
            amount: 100,
          })
        ).type,
        "ack",
      );
      assert.equal(
        (
          await cmd(a, {
            type: "approve",
            playerId: guest.playerId,
            amount: 100,
          })
        ).type,
        "ack",
      );
      await b.wait(
        (m) =>
          m.type === "state" &&
          m.state.players.every((p) => p.status === "active"),
      );
    });
    await t.test("duplicate commands do not double spend", async () => {
      const id = randomUUID(),
        revision = a.state.revision;
      assert.equal(
        (
          await cmd(
            a,
            { type: "buyin", playerId: host.playerId, amount: 20 },
            { id, revision },
          )
        ).type,
        "ack",
      );
      a.messages.length = 0;
      const duplicate = await cmd(
        a,
        { type: "buyin", playerId: host.playerId, amount: 20 },
        { id, revision },
      );
      assert.equal(duplicate.type, "ack");
      assert.equal(duplicate.duplicate, true);
      assert.equal(
        a.state.players.find((p) => p.id === host.playerId)?.stack,
        120,
      );
    });
    await t.test("stale revision and nonhost takeover rejected", async () => {
      const stale = await cmd(a, { type: "pause" }, { revision: 0 });
      assert.equal(stale.type, "error");
      assert.match(stale.error, /State changed/);
      await cmd(a, { type: "start" });
      assert.equal(
        (
          await cmd(b, {
            type: "act",
            playerId: host.playerId,
            takeover: true,
            action: "fold",
          })
        ).type,
        "error",
      );
    });
    await t.test(
      "cross-table credentials and foreign origins rejected",
      async () => {
        const other = await seat("create", { name: "Other" });
        const invalid = new Client();
        clients.push(invalid);
        await invalid.opened();
        const closed = new Promise<number>((resolve) => {
          invalid.socket.once("close", resolve);
        });
        invalid.send({ type: "auth", code: other.code, token: host.token });
        assert.equal(await closed, 4003);
        await new Promise<void>((resolve, reject) => {
          const foreign = new WebSocket(
            origin.replace("http:", "ws:") + "/ws",
            { origin: "https://evil.example" },
          );
          foreign.on("error", () => {
            resolve();
          });
          foreign.on("open", () => {
            foreign.terminate();
            reject(Error("Foreign origin accepted"));
          });
        });
      },
    );
    const before = structuredClone(a.state);
    clients.forEach((c) => {
      c.socket.close();
    });
    await stop();
    await launch();
    const resumed = await connect(host);
    await t.test("same seat, hand, revision and ledger survive restart", () => {
      assert.equal(resumed.state.you, host.playerId);
      assert.equal(resumed.state.hostId, host.playerId);
      assert.equal(resumed.state.revision, before.revision);
      assert.deepEqual(resumed.state.hand, before.hand);
      assert.deepEqual(resumed.state.ledger, before.ledger);
    });
    await t.test("undo works after restart", async () => {
      assert.equal((await cmd(resumed, { type: "undo" })).type, "ack");
      assert.equal(resumed.state.phase, "between");
      assert.equal(resumed.state.players[0]?.stack, 120);
    });
    await t.test(
      "public lobby lists minimal metadata and removes ended rooms live",
      async () => {
        const lobby = new Client();
        clients.push(lobby);
        await lobby.opened();
        lobby.send({ type: "lobby" });
        const first = await lobby.wait((m) => m.type === "lobby");
        if (first.type !== "lobby") throw Error("Expected lobby");
        const room = first.tables.find((x) => x.code === host.code);
        assert.equal(room?.title, "Test table");
        assert.equal(room.players, 2);
        assert.deepEqual(Object.keys(room).sort(), [
          "code",
          "phase",
          "players",
          "title",
        ]);
        assert.ok(!JSON.stringify(first).includes(host.token));
        assert.equal((await cmd(resumed, { type: "end" })).type, "ack");
        await lobby.wait(
          (m) =>
            m.type === "lobby" && !m.tables.some((x) => x.code === host.code),
        );
      },
    );
  } finally {
    clients.forEach((c) => {
      c.socket.terminate();
    });
    await stop();
    rmSync(directory, { recursive: true, force: true });
  }
});
