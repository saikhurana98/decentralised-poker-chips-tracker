import { z } from "zod";
const playerSchema = z.object({
  id: z.string(),
  name: z.string(),
  status: z.enum(["pending", "active", "out"]),
  seat: z.number().nullable(),
  stack: z.number(),
});
const handSchema = z.object({
  ids: z.array(z.string()),
  street: z.enum(["preflop", "flop", "turn", "river"]),
  turn: z.string().nullable(),
  folded: z.array(z.string()),
  acted: z.record(z.string(), z.number()),
  bets: z.record(z.string(), z.number()),
  contributions: z.record(z.string(), z.number()),
  current: z.number(),
  lastRaise: z.number(),
  awards: z.array(z.object({ playerId: z.string(), amount: z.number() })),
  sb: z.string(),
  bb: z.string(),
});
export const credentialSchema = z.object({
  code: z.string(),
  token: z.string(),
  playerId: z.string(),
});
export const credentialsSchema = z.record(z.string(), credentialSchema);
export const stateSchema = z.object({
  revision: z.number(),
  hostId: z.string(),
  title: z.string(),
  currency: z.string(),
  minorPerChip: z.number(),
  defaultBuyin: z.number(),
  players: z.array(playerSchema),
  small: z.number(),
  big: z.number(),
  phase: z.enum(["between", "playing", "deal", "showdown", "ended"]),
  paused: z.boolean(),
  handNo: z.number(),
  dealer: z.number().nullable(),
  hand: handSchema.nullable(),
  ledger: z.array(
    z.object({
      id: z.string(),
      playerId: z.string(),
      kind: z.enum(["buyin", "cashout"]),
      amount: z.number(),
      at: z.string(),
    }),
  ),
  log: z.array(z.object({ at: z.string(), text: z.string() })),
  undoCount: z.number(),
  code: z.string(),
  you: z.string(),
  pots: z.array(
    z.object({ amount: z.number(), eligible: z.array(z.string()) }),
  ),
  legal: z
    .object({
      toCall: z.number(),
      check: z.boolean(),
      minRaise: z.number(),
      maxRaise: z.number(),
      canRaise: z.boolean(),
    })
    .nullable(),
  balances: z.array(
    z.object({
      playerId: z.string(),
      name: z.string(),
      buyin: z.number(),
      cashout: z.number(),
      stack: z.number(),
      net: z.number(),
    }),
  ),
  settlement: z.array(
    z.object({ from: z.string(), to: z.string(), amount: z.number() }),
  ),
  online: z.array(z.string()),
});
export const serverMessageSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("state"), state: stateSchema }),
  z.object({
    type: z.literal("ack"),
    id: z.string(),
    revision: z.number(),
    duplicate: z.boolean(),
  }),
  z.object({
    type: z.literal("error"),
    id: z.string().optional(),
    error: z.string(),
  }),
  z.object({
    type: z.literal("lobby"),
    tables: z.array(
      z.object({
        code: z.string(),
        title: z.string(),
        players: z.number(),
        phase: z.string(),
      }),
    ),
  }),
]);
export type State = z.infer<typeof stateSchema>;
export type Player = z.infer<typeof playerSchema>;
export type Credential = z.infer<typeof credentialSchema>;
export const commandSchema = z.union([
  z.object({ type: z.enum(["start", "pause", "undo", "advance", "end"]) }),
  z.object({
    type: z.enum(["approve", "buyin", "cashout"]),
    playerId: z.string(),
    amount: z.number(),
  }),
  z.object({
    type: z.enum(["reject", "transfer", "remove"]),
    playerId: z.string(),
  }),
  z.object({ type: z.literal("seat"), playerId: z.string(), seat: z.number() }),
  z.object({ type: z.literal("blinds"), small: z.number(), big: z.number() }),
  z.object({ type: z.literal("award"), winners: z.array(z.array(z.string())) }),
  z.object({
    type: z.literal("act"),
    action: z.enum(["fold", "check", "call", "raise", "allin"]),
    amount: z.number().optional(),
    playerId: z.string().optional(),
    takeover: z.boolean().optional(),
  }),
]);

export type Command = z.infer<typeof commandSchema>;
