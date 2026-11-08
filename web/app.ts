import {
  commandSchema,
  credentialSchema,
  credentialsSchema,
  serverMessageSchema,
  type State,
  type Player,
  type Credential,
  type Command,
} from "./protocol";
function element<T extends Element>(selector: string, kind: { new (): T }): T {
  const found = document.querySelector(selector);
  if (!(found instanceof kind)) throw new Error(`Missing element ${selector}`);
  return found;
}
const app = element("#app", HTMLElement),
  status = element("#connection", HTMLElement);
const escape = (x: string | number | null | undefined) =>
  String(x ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ] ?? c,
  );
const fmt = (n: number) => n.toLocaleString("en-IN");
const cash = (n: number) =>
  new Intl.NumberFormat("en-IN", {
    style: "currency",
    currency: state?.currency || "INR",
  }).format((n * (state?.minorPerChip || 100)) / 100);
let state: State | null = null,
  ws: WebSocket | null = null,
  credential: Credential | null = null,
  retry: ReturnType<typeof setTimeout> | null = null,
  pending: {
    message: {
      type: "command";
      id: string;
      revision: number;
      command: Command;
    };
    sent: boolean;
  } | null = null,
  connected = false,
  mode = "create";
function getState(): State {
  if (!state) throw new Error("No table connected");
  return state;
}
let toastTimer: ReturnType<typeof setTimeout> | undefined;
const saved = () => {
  try {
    return credentialsSchema.parse(
      JSON.parse(localStorage.getItem("pokerbank-seats-v2") || "{}"),
    );
  } catch {
    return {};
  }
};
function remember(c: Credential) {
  const all = saved();
  all[c.code] = c;
  localStorage.setItem("pokerbank-seats-v2", JSON.stringify(all));
}
function toast(message: string) {
  const t = element("#toast", HTMLElement);
  t.textContent = message;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 6000);
}
const pname = (id: string | null) =>
  escape(getState().players.find((p) => p.id === id)?.name || "Player");
let lobbySocket: WebSocket | null = null,
  lobbyRetry: ReturnType<typeof setTimeout> | null = null;
function stopLobby() {
  clearTimeout(lobbyRetry ?? undefined);
  if (lobbySocket) {
    lobbySocket.onclose = null;
    lobbySocket.close();
    lobbySocket = null;
  }
}
function startLobby() {
  stopLobby();
  if (state || credential) return;
  const socket = (lobbySocket = new WebSocket(
    `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`,
  ));
  socket.onopen = () => {
    socket.send(JSON.stringify({ type: "lobby" }));
  };
  socket.onmessage = (e) => {
    const m = serverMessageSchema.parse(JSON.parse(String(e.data))),
      list = document.querySelector("#active-games");
    if (m.type !== "lobby" || !list) return;
    list.innerHTML = m.tables.length
      ? m.tables
          .map(
            (t) =>
              `<article class="active-game"><div><h3>${escape(t.title)}</h3><p>Room ID <strong class="code">${escape(t.code)}</strong> · ${t.players}/10 players · ${escape({ between: "Between hands", playing: "Playing", deal: "Dealing", showdown: "Showdown", paused: "Paused" }[t.phase] || t.phase)}</p></div><button data-join-room="${escape(t.code)}">Join →</button></article>`,
          )
          .join("")
      : '<p class="muted">No active games yet. Start a table and invite your friends.</p>';
  };
  socket.onclose = () => {
    if (!state && !credential) {
      const list = document.querySelector("#active-games");
      if (list) list.textContent = "Reconnecting to active games…";
      lobbyRetry = setTimeout(startLobby, 3000);
    }
  };
  socket.onerror = () => {};
}
function landing() {
  const recent = Object.values(saved()).slice(-5);
  const code = new URLSearchParams(location.search).get("table") || "";
  app.innerHTML = `<section class="hero"><div><p class="eyebrow">THE HOME GAME, IN SYNC</p><h1>All the chips.<br>One shared table.</h1><p class="lead">Keep the cards in your hands. Track every bet, side pot and buy-in together, on your own phones.</p><p class="feature">2–10 players &nbsp; · &nbsp; No accounts &nbsp; · &nbsp; Real cards</p></div><div class="panel"><div class="tabs"><button data-mode="create" class="${mode === "create" ? "primary" : ""}">New table</button><button data-mode="join" class="${mode === "join" ? "primary" : ""}">Join a table</button></div><form id="entry"><label>Your name<input name="name" maxlength="32" required autocomplete="nickname" placeholder="What should we call you?"></label>${mode === "create" ? `<label>Table name<input name="title" maxlength="32" value="Home game" required></label><div class="entry-grid"><label>Currency<select name="currency"><option>INR</option><option>USD</option><option>EUR</option><option>GBP</option></select></label><label>Value per chip<input name="chipValue" type="number" value="1" min="0.01" max="1000" step="0.01" required></label><label>Default buy-in (chips)<input name="defaultBuyin" type="number" value="1000" min="1" max="10000000" step="1" required></label></div>` : ""}${mode === "join" ? `<label>Table key<input name="code" value="${escape(code)}" minlength="8" maxlength="8" required placeholder="8-character key" autocapitalize="characters" spellcheck="false"></label>` : ""}<button class="primary" type="submit">${mode === "create" ? "Create your table" : "Request a seat"} →</button></form><p class="muted inline-1">${mode === "create" ? "You host. Invite friends, approve buy-ins, and deal real cards." : "Your host will approve your seat and starting chips between hands."}</p>${recent.map((c) => `<button class="resume" data-resume="${escape(c.code)}">Reconnect to ${escape(c.code)}</button>`).join("")}</div></section><section class="panel lobby"><h2>Active games</h2><p class="muted">Find your table and request a seat. The host approves all players.</p><div id="active-games">Loading active games…</div></section><footer>Chips and settlement tracking only. Payments happen outside Poker Bank.</footer>`;
  startLobby();
}
function connect(c: Credential) {
  stopLobby();
  credential = c;
  state = null;
  pending = null;
  history.replaceState(null, "", `/?table=${c.code}`);
  if (ws) {
    ws.onclose = null;
    ws.close();
  }
  clearTimeout(retry ?? undefined);
  openSocket();
}
function openSocket() {
  connected = false;
  status.textContent = "Connecting…";
  ws = new WebSocket(
    `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/ws`,
  );
  ws.onopen = () =>
    ws?.send(
      JSON.stringify({
        type: "auth",
        code: credential?.code,
        token: credential?.token,
      }),
    );
  ws.onmessage = (e) => {
    const m = serverMessageSchema.parse(JSON.parse(String(e.data)));
    if (m.type === "state") {
      state = m.state;
      connected = true;
      status.textContent = `● Live · table ${getState().code}`;
      render();
      if (pending && !pending.sent) {
        pending.sent = true;
        ws?.send(JSON.stringify(pending.message));
      }
    }
    if (m.type === "ack") {
      if (pending?.message.id === m.id) pending = null;
    }
    if (m.type === "error") {
      if (pending?.message.id === m.id) pending = null;
      toast(m.error);
      if (state) render();
    }
  };
  ws.onerror = () => {};
  ws.onclose = (e) => {
    connected = false;
    status.textContent = "Offline · reconnecting…";
    if (pending) pending.sent = false;
    if (state) render();
    if (e.code === 4003) {
      status.textContent = "Request a new seat";
      const all = saved();
      if (credential) Reflect.deleteProperty(all, credential.code);
      localStorage.setItem("pokerbank-seats-v2", JSON.stringify(all));
      state = null;
      credential = null;
      pending = null;
      mode = "join";
      landing();
      toast("This saved seat is unavailable. Request a new seat below.");
      return;
    }
    retry = setTimeout(openSocket, 2000);
  };
}
function command(raw: unknown) {
  const c = commandSchema.parse(raw);
  if (!connected || ws?.readyState !== 1) {
    toast("Wait for the connection to return");
    return;
  }
  if (pending) {
    toast("Waiting for the previous action");
    return;
  }
  pending = {
    message: {
      type: "command",
      id: crypto.randomUUID(),
      revision: getState().revision,
      command: c,
    },
    sent: true,
  };
  ws.send(JSON.stringify(pending.message));
  render();
}
function button(type: string, text: string, cls = "", disabled = false) {
  return `<button data-command="${type}" class="${cls}" ${disabled || pending || !connected ? "disabled" : ""}>${text}</button>`;
}
function render() {
  if (!state) return;
  const s = state,
    host = s.hostId === s.you,
    me = s.players.find((p) => p.id === s.you),
    h = s.hand,
    busy = s.phase !== "between",
    funded = s.players.filter(
      (p) => p.status === "active" && p.stack > 0,
    ).length,
    total = Object.values(h?.contributions || {}).reduce((a, b) => a + b, 0),
    turn = s.players.find((p) => p.id === h?.turn),
    stage = {
      between: "Between hands",
      playing: h?.street,
      deal: "Ready to deal",
      showdown: "Showdown",
      ended: "Session complete",
    }[s.phase];
  const seat = (p: Player) => {
    const h = s.hand;
    const inhand = h !== null && h.ids.includes(p.id) && busy;
    const folded = inhand && h.folded.includes(p.id);
    return `<article class="seat ${h?.turn === p.id && s.phase === "playing" ? "turn" : ""} ${folded ? "folded" : ""}"><div class="seat-top"><span>SEAT ${p.seat} ${p.id === s.you ? "· YOU" : ""}</span><span class="online ${s.online.includes(p.id) ? "yes" : ""}" title="${s.online.includes(p.id) ? "Online" : "Disconnected"}"></span></div><h3>${escape(p.name)}${s.dealer === p.seat ? '<span class="marker">D</span>' : ""}${inhand && h.sb === p.id ? '<span class="marker">S</span>' : ""}${inhand && h.bb === p.id ? '<span class="marker">B</span>' : ""}</h3><div class="stack">${fmt(p.stack)}</div><small>${folded ? "Folded" : inhand && p.stack === 0 ? "All-in" : inhand && h.bets[p.id] ? `Bet ${fmt(h.bets[p.id] ?? 0)}` : p.stack === 0 ? "Needs a rebuy" : p.id === s.hostId ? "Host" : "Ready"}</small></article>`;
  };
  if (!me) return;
  const seats = Array.from({ length: 10 }, (_, i) => {
    const p = s.players.find((p) => p.status === "active" && p.seat === i + 1);
    return p
      ? seat(p)
      : `<div class="seat empty"><small>Seat ${i + 1}</small></div>`;
  });
  app.innerHTML = `<div class="bar"><div><p class="eyebrow">${s.phase === "ended" ? "THANKS FOR PLAYING" : "YOUR HOME GAME"} · ${host ? "HOST" : "PLAYER"}</p><h2>${escape(s.title || "Home game")} <span class="code">${s.code}</span></h2></div><div class="actions"><button id="invite">Invite friends ↗</button><button id="switch" class="text-button">Switch table</button></div></div>${!connected ? '<div class="notice error-text">Connection lost. Actions will resume after reconnecting.</div>' : ""}${s.paused ? '<div class="notice">Table paused by the host.</div>' : ""}${me.status === "pending" ? '<div class="notice">Your seat is waiting for host approval. You’ll join between hands.</div>' : ""}<div class="layout"><div><section class="table-wrap"><div class="seats">${seats.slice(0, 5).join("")}</div><div class="center"><h3>${escape(stage)}</h3><div class="pot">${fmt(total)} <span class="muted inline-2">in pot</span></div><p>${s.phase === "playing" ? `${pname(h?.turn ?? null)}${h?.turn === s.you ? " · your turn" : " to act"}` : s.phase === "deal" ? "The host will confirm the next community cards" : s.phase === "showdown" ? "Show your physical cards · host confirms winners" : s.phase === "ended" ? "Final balances are ready below" : h?.awards.length ? h.awards.map((a) => `${pname(a.playerId)} +${fmt(a.amount)}`).join(" · ") : "Invite your friends, take a seat, and start a hand."}</p><div class="summary"><span>HAND<strong>${s.handNo || "—"}</strong></span><span>BLINDS<strong>${fmt(s.small)} / ${fmt(s.big)}</strong></span><span>SEATED<strong>${s.players.filter((p) => p.status === "active").length} / 10</strong></span></div></div><div class="seats">${seats.slice(5).join("")}</div></section>${controls(host, turn)}${s.phase === "showdown" && host ? `<section class="controls"><h3>Confirm pot winners</h3><p class="muted">Select every tied winner for each pot. Odd chips go clockwise from the dealer.</p><form class="winners" id="winners">${s.pots.map((pot, i) => `<fieldset><legend>${i === 0 ? "Main pot" : `Side pot ${i}`} · ${fmt(pot.amount)}</legend>${pot.eligible.map((id) => `<label><input type="checkbox" name="pot${i}" value="${id}" ${pot.eligible.length === 1 ? "checked" : ""}>${pname(id)}</label>`).join("")}</fieldset>`).join("")}<button class="primary" ${pending || s.paused || !connected ? "disabled" : ""}>Award pots</button></form></section>` : ""}</div><aside class="rail"><section class="panel"><h3>${host ? "Host desk" : "At this table"}</h3><p class="muted">${host ? "You approve seats, confirm real-world buy-ins and choose showdown winners." : "Your own bets are on your phone. Your host manages buy-ins and payouts."}</p>${host ? `<div class="actions">${button("start", s.handNo ? "Next hand →" : "Start first hand →", "primary", s.phase !== "between" || funded < 2 || s.paused)}${button("pause", s.paused ? "Resume" : "Pause", "", s.phase === "ended")}${button("undo", "Undo game action", "", !s.undoCount || s.phase === "ended")}</div>${s.phase === "deal" ? `<p class="inline-5">Deal the ${h?.street === "preflop" ? "flop" : h?.street === "flop" ? "turn" : "river"} using your physical cards.</p>${button("advance", "Cards dealt · continue", "primary", s.paused)}` : ""}` : `<p>Host: <strong>${pname(s.hostId)}</strong></p>`}</section><section class="panel"><h3>Seat requests <span class="badge">${s.players.filter((p) => p.status === "pending").length}</span></h3>${
    s.players
      .filter((p) => p.status === "pending")
      .map(
        (p) =>
          `<div class="request"><p>${escape(p.name)} ${p.id === s.you ? "(you)" : ""}</p>${host ? `<button data-approve="${p.id}" ${busy || pending ? "disabled" : ""}>Approve & buy in</button>${p.id !== s.hostId ? ` <button data-reject="${p.id}">Decline</button>` : ""}` : "<small>Awaiting host approval</small>"}</div>`,
      )
      .join("") || '<p class="muted">Everyone is accounted for.</p>'
  }${busy ? "<small>New seats and buy-ins open after this hand.</small>" : ""}</section>${
    host
      ? `<section class="panel"><details><summary>Table management</summary><label>Player<select id="manage-player">${s.players
          .filter((p) => p.status === "active")
          .map(
            (p) =>
              `<option value="${p.id}">${escape(p.name)} · seat ${p.seat}</option>`,
          )
          .join(
            "",
          )}</select></label><div class="actions"><button data-manage="buyin" ${busy ? "disabled" : ""}>Rebuy</button><button data-manage="cashout" ${busy ? "disabled" : ""}>Cash out</button><button data-manage="seat" ${busy ? "disabled" : ""}>Move seat</button><button data-manage="transfer" ${s.phase === "ended" ? "disabled" : ""}>Make host</button><button data-manage="remove" ${busy ? "disabled" : ""}>Remove seat</button></div><div class="actions inline-3">${button("blinds", "Change blinds", "", busy)}${button("end", "End & cash out all", "danger", busy)}</div><p class="muted inline-4">Cash entries are permanent. Undo affects gameplay only.</p></details></section>`
      : ""
  }</aside></div><div class="below"><section class="panel"><details ${s.phase === "ended" ? "open" : ""}><summary>Cash ledger & settlement</summary><p class="muted">${cash(1)} per chip. Money values follow your table’s chip value. Confirm actual payments outside this app. Net includes live stacks.</p><div class="scroll"><table><thead><tr><th>Player</th><th>Buy-in</th><th>Out</th><th>Stack</th><th>Net chips / money</th></tr></thead><tbody>${s.balances.map((b) => `<tr><td>${escape(b.name)}</td><td>${fmt(b.buyin)}</td><td>${fmt(b.cashout)}</td><td>${fmt(b.stack)}</td><td class="${b.net >= 0 ? "positive" : "negative"}">${b.net > 0 ? "+" : ""}${fmt(b.net)}<br>${escape(cash(b.net))}</td></tr>`).join("")}</tbody></table></div>${s.phase === "between" || s.phase === "ended" ? `<h3 class="inline-6">Suggested settlement</h3>${s.settlement.map((x) => `<p>${pname(x.from)} → ${pname(x.to)} <strong>${escape(cash(x.amount))}</strong> <small>(${fmt(x.amount)} chips)</small></p>`).join("") || '<p class="muted">All square.</p>'}` : '<p class="muted">Settlement appears after this hand.</p>'}<button id="export">Export ledger</button><details class="inline-6"><summary>Cash entries (${s.ledger.length})</summary><div class="audit">${s.ledger.map((e) => `<p>${pname(e.playerId)} · ${e.kind} · ${fmt(e.amount)} chips · ${escape(cash(e.amount))}<br><time>${escape(new Date(e.at).toLocaleString())}</time></p>`).join("")}</div></details></details></section><section class="panel"><details open><summary>Table activity</summary><div class="audit">${
    [...s.log]
      .reverse()
      .map(
        (e) =>
          `<p><time>${escape(new Date(e.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }))}</time>${escape(e.text)}</p>`,
      )
      .join("") || "<p>Your table story starts here.</p>"
  }</div></details></section></div><footer>Physical cards. Whole chips. Every change synced. · Keep this browser’s site data to reconnect to your seat.</footer>`;
}
function controls(host: boolean, turn: Player | undefined) {
  const s = getState(),
    r = s.legal;
  if (r)
    return `<section class="controls your-controls"><div class="control-title"><strong>Your turn</strong><span class="muted">${r.toCall ? `Call ${fmt(r.toCall)}` : "You can check"}</span></div><div class="actions"><button data-action="fold" class="danger" ${pending ? "disabled" : ""}>Fold</button><button data-action="${r.check ? "check" : "call"}" class="primary" ${pending ? "disabled" : ""}>${r.check ? "Check" : `Call ${fmt(r.toCall)}`}</button><button data-action="allin" ${pending || (!r.canRaise && r.maxRaise > (s.hand?.current ?? 0)) ? "disabled" : ""}>All-in</button></div>${r.canRaise && r.maxRaise >= r.minRaise ? `<form id="raise" class="raise-row"><label for="raise-total">Raise to</label><input id="raise-total" type="number" min="${r.minRaise}" max="${r.maxRaise}" step="1" value="${r.minRaise}" required><button ${pending ? "disabled" : ""}>Raise</button></form>` : ""}</section>`;
  if (s.phase === "playing" && turn)
    return `<section class="controls"><p>${s.paused ? "Table paused" : `Waiting for ${pname(turn.id)}`}${!s.online.includes(turn.id) ? " · disconnected" : ""}</p>${host ? `<button id="takeover">Control this turn as host</button><small> Only act with the player’s agreement.</small>` : ""}</section>`;
  return "";
}
function askAmount(message: string, initial = "1000"): number | null {
  const v = prompt(message, initial);
  if (v === null) return null;
  const n = Number(v);
  if (!Number.isSafeInteger(n) || n <= 0 || n > 10000000) {
    toast("Enter a whole amount between 1 and 10,000,000");
    return null;
  }
  return n;
}
async function submit(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const form = event.target;
  if (!(form instanceof HTMLFormElement)) return;
  if (form.id === "entry") {
    const b = form.querySelector("button");
    if (!b) return;
    b.disabled = true;
    try {
      const data = new FormData(form);
      const response = await fetch(`/api/${mode}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ...Object.fromEntries(data),
          ...(mode === "create"
            ? {
                minorPerChip: Math.round(Number(data.get("chipValue")) * 100),
                defaultBuyin: Number(data.get("defaultBuyin")),
              }
            : {}),
        }),
      });
      const body: unknown = await response.json();
      if (!response.ok)
        throw new Error(
          "Unable to join or create table. Check your details and try again.",
        );
      const seat = credentialSchema.parse(body);
      remember(seat);
      connect(seat);
    } catch (error) {
      toast(error instanceof Error ? error.message : "Request failed");
      b.disabled = false;
    }
    return;
  }
  if (form.id === "raise")
    command({
      type: "act",
      action: "raise",
      amount: Number(element("#raise-total", HTMLInputElement).value),
    });
  if (form.id === "winners") {
    const data = new FormData(form),
      winners = getState().pots.map((_, i) =>
        data.getAll("pot" + String(i)).map(String),
      );
    if (winners.some((x) => !x.length)) {
      toast("Choose a winner for every pot");
      return;
    }
    if (confirm("Confirm these winners and distribute every pot?"))
      command({ type: "award", winners });
  }
}
app.addEventListener("submit", (event) => {
  void submit(event).catch((error: unknown) => {
    toast(error instanceof Error ? error.message : "Request failed");
  });
});
app.addEventListener("click", (event) => {
  if (!(event.target instanceof Element)) return;
  const b = event.target.closest("button");
  if (!b || b.disabled) return;
  const room = b.dataset["joinRoom"];
  if (room) {
    const seat = saved()[room];
    if (seat) {
      connect(seat);
      return;
    }
    history.replaceState(null, "", `/?table=${room}`);
    mode = "join";
    landing();
    element("[name=name]", HTMLInputElement).focus();
    return;
  }
  if (b.dataset["mode"]) {
    mode = b.dataset["mode"];
    landing();
    return;
  }
  const resume = b.dataset["resume"];
  if (resume) {
    const seat = saved()[resume];
    if (seat) connect(seat);
    return;
  }
  if (b.id === "switch") {
    if (ws) {
      ws.onclose = null;
      ws.close();
    }
    clearTimeout(retry ?? undefined);
    state = null;
    credential = null;
    connected = false;
    pending = null;
    history.replaceState(null, "", "/");
    status.textContent = "Your next home game";
    landing();
    return;
  }
  if (!state) return;
  const s = getState();
  if (b.id === "invite") {
    element("#qr", HTMLImageElement).src = "/qr?code=" + s.code;
    element("#share-code", HTMLElement).textContent = s.code;
    element("#share-link", HTMLInputElement).value =
      location.origin + "/?table=" + s.code;
    element("#share", HTMLDialogElement).showModal();
  }
  const approve = b.dataset["approve"];
  if (approve) {
    const n = askAmount(
      `Starting chips — confirm funds received (${cash(s.defaultBuyin)})`,
      String(s.defaultBuyin),
    );
    if (n) command({ type: "approve", playerId: approve, amount: n });
  }
  if (b.dataset["reject"])
    command({ type: "reject", playerId: b.dataset["reject"] });
  const action = b.dataset["action"];
  if (action) {
    if (
      action === "allin" &&
      !confirm("Move your whole remaining stack all-in?")
    )
      return;
    command({ type: "act", action });
  }
  const type = b.dataset["command"];
  if (type) {
    if (
      type === "end" &&
      !confirm("End session and record all remaining stacks as cashed out?")
    )
      return;
    if (
      type === "undo" &&
      !confirm("Undo the last game action? Cash ledger stays unchanged.")
    )
      return;
    if (type === "blinds") {
      const small = askAmount("Small blind", String(s.small));
      if (!small) return;
      const big = askAmount("Big blind", String(s.big));
      if (big) command({ type, small, big });
    } else command({ type });
  }
  const manage = b.dataset["manage"];
  if (manage) {
    const playerId = element("#manage-player", HTMLSelectElement).value;
    if (!playerId) return;
    const c: {
      type: string;
      playerId: string;
      amount?: number;
      seat?: number;
    } = { type: manage, playerId };
    if (manage === "buyin" || manage === "cashout") {
      const n = askAmount(
        manage === "buyin"
          ? "Rebuy chips — confirm funds received"
          : "Cash out chips — record amount returned",
      );
      if (!n) return;
      c.amount = n;
    }
    if (manage === "seat") {
      const n = askAmount("Seat number (1–10), swaps occupied seats", "1");
      if (!n) return;
      c.seat = n;
    }
    if (
      manage === "transfer" &&
      !confirm("Transfer host control to this player?")
    )
      return;
    if (manage === "remove" && !confirm("Remove this empty seat?")) return;
    command(c);
  }
  if (b.id === "takeover") {
    const hand = s.hand;
    if (!hand?.turn) return;
    const player = s.players.find((p) => p.id === hand.turn);
    if (!player) return;
    const action = prompt(
      `Host controls ${player.name}. Enter fold, check, call, raise, or allin.`,
    );
    if (
      !action ||
      !["fold", "check", "call", "raise", "allin"].includes(action)
    )
      return;
    const c: {
      type: string;
      playerId: string;
      takeover: boolean;
      action: string;
      amount?: number;
    } = { type: "act", playerId: hand.turn, takeover: true, action };
    if (action === "raise") {
      const n = askAmount("Raise to total chips this round");
      if (!n) return;
      c.amount = n;
    }
    if (confirm("Record this action on behalf of the player?")) command(c);
  }
  if (b.id === "export") {
    const blob = new Blob(
        [
          JSON.stringify(
            {
              code: s.code,
              title: s.title,
              currency: s.currency,
              minorPerChip: s.minorPerChip,
              exported: new Date().toISOString(),
              balances: s.balances,
              ledger: s.ledger,
              settlement: s.settlement,
              log: s.log,
            },
            null,
            2,
          ),
        ],
        { type: "application/json" },
      ),
      a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `poker-bank-${s.code}.json`;
    a.click();
    setTimeout(() => {
      URL.revokeObjectURL(a.href);
    }, 1000);
  }
});
element("#close-share", HTMLButtonElement).onclick = () => {
  element("#share", HTMLDialogElement).close();
};
element("#copy-link", HTMLButtonElement).onclick = () => {
  void navigator.clipboard
    .writeText(element("#share-link", HTMLInputElement).value)
    .then(() => {
      toast("Invite link copied");
    })
    .catch(() => {
      element("#share-link", HTMLInputElement).select();
      toast("Select and copy the invite link");
    });
};
const table = new URLSearchParams(location.search).get("table")?.toUpperCase();
if (table) mode = "join";
const existing = table ? saved()[table] : undefined;
if (existing) connect(existing);
else landing();
