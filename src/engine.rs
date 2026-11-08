use std::collections::{BTreeMap, BTreeSet};

use chrono::{SecondsFormat, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::model::{
    AuditEntry, Award, Balance, BetAction, CashEntry, CashKind, Command, CreateRequest, GameState,
    Hand, HistoryEntry, LegalActions, MAX_CHIPS, MAX_SAFE_INTEGER, Phase, Player, PlayerStatus,
    Pot, Settlement, Street,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct RuleError(pub String);

pub type Result<T> = std::result::Result<T, RuleError>;

pub fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(RuleError(message.into()))
    }
}

pub fn amount(value: u64) -> Result<u64> {
    ensure(
        (1..=MAX_CHIPS).contains(&value),
        "Use a whole chip amount from 1 to 10,000,000",
    )?;
    Ok(value)
}

pub fn name(value: &str) -> Result<String> {
    let value = value.trim();
    // Browser JavaScript string length counts UTF-16 code units, not bytes or scalar values.
    ensure(
        (1..=32).contains(&value.encode_utf16().count()),
        "Name must be 1–32 characters",
    )?;
    Ok(value.to_owned())
}

pub fn fresh(host_id: String, request: &CreateRequest) -> Result<GameState> {
    ensure(
        (1..=100_000).contains(&request.minor_per_chip),
        "Chip value must be 0.01–1000",
    )?;
    amount(request.default_buyin)?;
    Ok(GameState {
        revision: 0,
        host_id: host_id.clone(),
        title: name(&request.title)?,
        currency: request.currency,
        minor_per_chip: request.minor_per_chip,
        default_buyin: request.default_buyin,
        players: vec![Player {
            id: host_id,
            name: name(&request.name)?,
            status: PlayerStatus::Pending,
            seat: None,
            stack: 0,
        }],
        small: 5,
        big: 10,
        phase: Phase::Between,
        paused: false,
        hand_no: 0,
        dealer: None,
        hand: None,
        ledger: Vec::new(),
        history: Vec::new(),
        log: Vec::new(),
    })
}

fn player<'a>(state: &'a GameState, id: &str) -> Result<&'a Player> {
    state
        .players
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| RuleError("Unknown player".into()))
}

fn player_mut<'a>(state: &'a mut GameState, id: &str) -> Result<&'a mut Player> {
    state
        .players
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| RuleError("Unknown player".into()))
}

fn hand(state: &GameState) -> Result<&Hand> {
    state
        .hand
        .as_ref()
        .ok_or_else(|| RuleError("No active hand".into()))
}

fn hand_mut(state: &mut GameState) -> Result<&mut Hand> {
    state
        .hand
        .as_mut()
        .ok_or_else(|| RuleError("No active hand".into()))
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn audit(state: &mut GameState, text: impl Into<String>) {
    state.log.push(AuditEntry {
        at: now(),
        text: text.into(),
    });
    if state.log.len() > 200 {
        state.log.drain(..state.log.len() - 200);
    }
}

fn money(state: &mut GameState, id: &str, kind: CashKind, value: u64) -> Result<()> {
    amount(value)?;
    let p = player(state, id)?;
    match kind {
        CashKind::Cashout => ensure(p.stack >= value, "Not enough chips")?,
        CashKind::Buyin => ensure(
            state.players.iter().map(|p| p.stack).sum::<u64>() + value <= MAX_CHIPS,
            "Table chip limit is 10,000,000",
        )?,
    }
    // Ledger totals must remain exactly representable by existing JavaScript clients.
    let lifetime = state
        .ledger
        .iter()
        .filter(|e| e.player_id == id && e.kind == kind)
        .try_fold(0u64, |sum, e| sum.checked_add(e.amount));
    ensure(
        lifetime
            .and_then(|n| n.checked_add(value))
            .is_some_and(|n| n <= MAX_SAFE_INTEGER - MAX_CHIPS),
        "Lifetime ledger limit reached",
    )?;
    let p = player_mut(state, id)?;
    match kind {
        CashKind::Buyin => p.stack += value,
        CashKind::Cashout => p.stack -= value,
    }
    let display = p.name.clone();
    state.ledger.push(CashEntry {
        id: Uuid::new_v4().to_string(),
        player_id: id.to_owned(),
        kind,
        amount: value,
        at: now(),
    });
    audit(state, format!("{display}: {} {value}", kind.label()));
    Ok(())
}

fn pay(state: &mut GameState, id: &str, value: u64) -> Result<()> {
    let p = player_mut(state, id)?;
    let paid = value.min(p.stack);
    p.stack -= paid;
    let h = hand_mut(state)?;
    *h.bets.entry(id.to_owned()).or_default() += paid;
    *h.contributions.entry(id.to_owned()).or_default() += paid;
    Ok(())
}

fn refund(state: &mut GameState) -> Result<()> {
    let mut entries = hand(state)?
        .contributions
        .iter()
        .map(|(id, n)| (id.clone(), *n))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    if let Some((id, value)) = entries.first() {
        let second = entries.get(1).map_or(0, |e| e.1);
        if *value > second {
            let excess = value - second;
            let p = player_mut(state, id)?;
            p.stack += excess;
            let display = p.name.clone();
            let h = hand_mut(state)?;
            h.contributions.insert(id.clone(), second);
            let bet = h.bets.get(id).copied().unwrap_or_default();
            h.bets.insert(id.clone(), bet.saturating_sub(excess));
            audit(state, format!("{display}: uncalled {excess} returned"));
        }
    }
    Ok(())
}

pub fn pots(state: &GameState) -> Vec<Pot> {
    let Some(h) = &state.hand else {
        return Vec::new();
    };
    let levels = h
        .contributions
        .values()
        .copied()
        .filter(|n| *n > 0)
        .collect::<BTreeSet<_>>();
    let mut previous = 0;
    levels
        .into_iter()
        .map(|level| {
            let contributors = h
                .ids
                .iter()
                .filter(|id| h.contributions.get(*id).copied().unwrap_or_default() >= level)
                .collect::<Vec<_>>();
            let result = Pot {
                amount: contributors
                    .iter()
                    .fold(0u64, |sum, _| sum + level - previous),
                eligible: contributors
                    .into_iter()
                    .filter(|id| !h.folded.contains(id))
                    .cloned()
                    .collect(),
            };
            previous = level;
            result
        })
        .collect()
}

fn live(state: &GameState) -> Result<Vec<String>> {
    let h = hand(state)?;
    Ok(h.ids
        .iter()
        .filter(|id| !h.folded.contains(id))
        .cloned()
        .collect())
}

fn clockwise(state: &GameState, ids: &[String], after: u8) -> Result<Vec<String>> {
    let mut ordered = ids
        .iter()
        .map(|id| {
            let seat = player(state, id)?
                .seat
                .ok_or_else(|| RuleError("Player has no seat".into()))?;
            let distance = (u16::from(seat) + 10 - u16::from(after)) % 10;
            Ok((if distance == 0 { 10 } else { distance }, id.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by_key(|p| p.0);
    Ok(ordered.into_iter().map(|(_, id)| id).collect())
}

fn finish(state: &mut GameState) -> Result<()> {
    state.phase = Phase::Between;
    hand_mut(state)?.turn = None;
    audit(state, format!("Hand {} completed", state.hand_no));
    Ok(())
}

fn settle_fold(state: &mut GameState) -> Result<()> {
    refund(state)?;
    let winner = live(state)?
        .into_iter()
        .next()
        .ok_or_else(|| RuleError("No eligible winner".into()))?;
    let total = hand(state)?.contributions.values().sum::<u64>();
    let p = player_mut(state, &winner)?;
    p.stack += total;
    let display = p.name.clone();
    let h = hand_mut(state)?;
    h.awards = vec![Award {
        player_id: winner,
        amount: total,
    }];
    h.contributions.clear();
    audit(state, format!("{display} wins {total}"));
    finish(state)
}

fn needs_action(state: &GameState, id: &str) -> Result<bool> {
    let h = hand(state)?;
    Ok(player(state, id)?.stack > 0
        && !h.folded.iter().any(|p| p == id)
        && (!h.acted.contains_key(id) || h.bets.get(id).copied().unwrap_or_default() < h.current))
}

fn progress(state: &mut GameState, after: u8) -> Result<()> {
    let alive = live(state)?;
    if alive.len() == 1 {
        return settle_fold(state);
    }
    let actors = alive
        .iter()
        .filter_map(|id| {
            state
                .players
                .iter()
                .find(|p| &p.id == id && p.stack > 0)
                .map(|p| p.id.clone())
        })
        .collect::<Vec<_>>();
    if let [actor] = actors.as_slice() {
        let h = hand_mut(state)?;
        h.current = alive
            .iter()
            .filter_map(|id| h.bets.get(id))
            .copied()
            .max()
            .unwrap_or_default();
        if h.bets.get(actor).copied().unwrap_or_default() >= h.current {
            h.acted.insert(actor.clone(), h.current);
        }
    }
    let ids = hand(state)?.ids.clone();
    let ordered = clockwise(state, &ids, after)?;
    let next = ordered
        .into_iter()
        .map(|id| needs_action(state, &id).map(|needed| (id, needed)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .find_map(|(id, needed)| needed.then_some(id));
    hand_mut(state)?.turn = next.clone();
    if next.is_some() {
        return Ok(());
    }
    if actors.len() < 2 || hand(state)?.street == Street::River {
        refund(state)?;
        state.phase = Phase::Showdown;
        audit(state, "Showdown: host confirms winners for each pot");
    } else {
        state.phase = Phase::Deal;
        let street = hand(state)?
            .street
            .next()
            .ok_or_else(|| RuleError("No further street".into()))?;
        audit(state, format!("Betting complete: deal {}", street.label()));
    }
    Ok(())
}

pub fn legal(state: &GameState, id: &str) -> Option<LegalActions> {
    let h = state.hand.as_ref()?;
    if state.phase != Phase::Playing || state.paused || h.turn.as_deref() != Some(id) {
        return None;
    }
    let p = state.players.iter().find(|p| p.id == id)?;
    let bet = h.bets.get(id).copied().unwrap_or_default();
    let to_call = h.current.saturating_sub(bet);
    Some(LegalActions {
        to_call: to_call.min(p.stack),
        check: to_call == 0,
        min_raise: h.current + h.last_raise,
        max_raise: bet + p.stack,
        can_raise: h
            .acted
            .get(id)
            .is_none_or(|acted| h.current.saturating_sub(*acted) >= h.last_raise),
    })
}

fn require_host(state: &GameState, actor: &str) -> Result<()> {
    ensure(state.host_id == actor, "Host only")
}
fn require_between(state: &GameState) -> Result<()> {
    ensure(
        state.phase == Phase::Between,
        "Wait until the hand is complete",
    )
}

pub fn apply(original: &GameState, actor: &str, command: &Command) -> Result<GameState> {
    let mut state = original.clone();
    ensure(state.phase != Phase::Ended, "Session has ended")?;
    player(&state, actor)?;
    if command.is_gameplay() {
        ensure(!state.paused, "Table is paused")?;
        state.history.push(HistoryEntry {
            phase: state.phase,
            hand_no: state.hand_no,
            dealer: state.dealer,
            hand: state.hand.clone(),
            stacks: state
                .players
                .iter()
                .map(|p| (p.id.clone(), p.stack))
                .collect(),
            log: state.log.clone(),
        });
        if state.history.len() > 50 {
            state.history.remove(0);
        }
    }
    match command {
        Command::Approve { player_id, amount } => {
            require_host(&state, actor)?;
            require_between(&state)?;
            ensure(
                player(&state, player_id)?.status == PlayerStatus::Pending,
                "Already approved",
            )?;
            ensure(
                state
                    .players
                    .iter()
                    .filter(|p| p.status == PlayerStatus::Active)
                    .count()
                    < 10,
                "Table has 10 seats",
            )?;
            let seat = (1..=10)
                .find(|seat| {
                    !state
                        .players
                        .iter()
                        .any(|p| p.status == PlayerStatus::Active && p.seat == Some(*seat))
                })
                .ok_or_else(|| RuleError("Table has 10 seats".into()))?;
            let p = player_mut(&mut state, player_id)?;
            p.seat = Some(seat);
            p.status = PlayerStatus::Active;
            money(&mut state, player_id, CashKind::Buyin, *amount)?;
            state.history.clear();
        }
        Command::Reject { player_id } => {
            require_host(&state, actor)?;
            ensure(
                player(&state, player_id)?.status == PlayerStatus::Pending
                    && *player_id != state.host_id,
                "Cannot remove this player",
            )?;
            state.players.retain(|p| &p.id != player_id);
        }
        Command::Buyin { player_id, amount } | Command::Cashout { player_id, amount } => {
            require_host(&state, actor)?;
            require_between(&state)?;
            ensure(
                player(&state, player_id)?.status == PlayerStatus::Active,
                "Player needs a seat",
            )?;
            let kind = if matches!(command, Command::Buyin { .. }) {
                CashKind::Buyin
            } else {
                CashKind::Cashout
            };
            money(&mut state, player_id, kind, *amount)?;
            state.history.clear();
        }
        Command::Seat { player_id, seat } => {
            require_host(&state, actor)?;
            require_between(&state)?;
            let p = player(&state, player_id)?;
            ensure(p.status == PlayerStatus::Active, "Player needs a seat")?;
            ensure((1..=10).contains(seat), "Seat must be 1–10")?;
            let previous = p.seat;
            if let Some(other) = state
                .players
                .iter_mut()
                .find(|p| p.status == PlayerStatus::Active && p.seat == Some(*seat))
            {
                other.seat = previous;
            }
            let p = player_mut(&mut state, player_id)?;
            p.seat = Some(*seat);
            let display = p.name.clone();
            state.history.clear();
            audit(&mut state, format!("{display} moved to seat {seat}"));
        }
        Command::Remove { player_id } => {
            require_host(&state, actor)?;
            require_between(&state)?;
            let p = player(&state, player_id)?;
            ensure(
                p.stack == 0 && *player_id != state.host_id,
                "Cash out first; transfer host before removing host",
            )?;
            let p = player_mut(&mut state, player_id)?;
            p.status = PlayerStatus::Out;
            p.seat = None;
            state.history.clear();
        }
        Command::Blinds { small, big } => {
            require_host(&state, actor)?;
            require_between(&state)?;
            amount(*small)?;
            amount(*big)?;
            ensure(
                *big >= small * 2,
                "Big blind must be at least twice small blind",
            )?;
            state.small = *small;
            state.big = *big;
            state.history.clear();
            audit(&mut state, format!("Blinds {small} / {big}"));
        }
        Command::Transfer { player_id } => {
            require_host(&state, actor)?;
            let p = player(&state, player_id)?;
            ensure(p.status != PlayerStatus::Out, "Unavailable player")?;
            let display = p.name.clone();
            state.host_id.clone_from(player_id);
            audit(&mut state, format!("{display} is now host"));
        }
        Command::Pause => {
            require_host(&state, actor)?;
            state.paused = !state.paused;
            let text = if state.paused {
                "Table paused"
            } else {
                "Table resumed"
            };
            audit(&mut state, text);
        }
        Command::Undo => {
            require_host(&state, actor)?;
            let previous = state
                .history
                .pop()
                .ok_or_else(|| RuleError("No game action to undo".into()))?;
            state.phase = previous.phase;
            state.hand_no = previous.hand_no;
            state.dealer = previous.dealer;
            state.hand = previous.hand;
            for (id, stack) in previous.stacks {
                if let Some(p) = state.players.iter_mut().find(|p| p.id == id) {
                    p.stack = stack;
                }
            }
            audit(
                &mut state,
                "Host undid the previous game action (cash ledger unchanged)",
            );
        }
        Command::Start => {
            require_host(&state, actor)?;
            require_between(&state)?;
            start(&mut state)?;
        }
        Command::Act {
            action,
            amount,
            player_id,
            takeover,
        } => {
            bet(
                &mut state,
                actor,
                *action,
                *amount,
                player_id.as_deref(),
                *takeover,
            )?;
        }
        Command::Advance => {
            require_host(&state, actor)?;
            ensure(state.phase == Phase::Deal, "Betting is not complete")?;
            let big = state.big;
            let dealer = state
                .dealer
                .ok_or_else(|| RuleError("Missing dealer".into()))?;
            let h = hand_mut(&mut state)?;
            h.street = h
                .street
                .next()
                .ok_or_else(|| RuleError("No further street".into()))?;
            h.current = 0;
            h.last_raise = big;
            h.acted.clear();
            h.bets = h.ids.iter().map(|id| (id.clone(), 0)).collect();
            let street = h.street;
            state.phase = Phase::Playing;
            audit(&mut state, format!("{} dealt", street.label()));
            progress(&mut state, dealer)?;
        }
        Command::Award { winners } => {
            require_host(&state, actor)?;
            award(&mut state, winners)?;
        }
        Command::End => {
            require_host(&state, actor)?;
            require_between(&state)?;
            let funded = state
                .players
                .iter()
                .filter(|p| p.stack > 0)
                .map(|p| (p.id.clone(), p.stack))
                .collect::<Vec<_>>();
            for (id, stack) in funded {
                money(&mut state, &id, CashKind::Cashout, stack)?;
            }
            state.phase = Phase::Ended;
            state.history.clear();
            audit(&mut state, "Session ended · all stacks cashed out");
        }
    }
    state.revision = state
        .revision
        .checked_add(1)
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| RuleError("Revision limit reached".into()))?;
    Ok(state)
}

fn start(state: &mut GameState) -> Result<()> {
    let mut funded = state
        .players
        .iter()
        .filter(|p| p.status == PlayerStatus::Active && p.stack > 0)
        .map(|p| {
            Ok((
                p.id.clone(),
                p.seat
                    .ok_or_else(|| RuleError("Player has no seat".into()))?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    funded.sort_by_key(|p| p.1);
    ensure(funded.len() >= 2, "Need at least two funded seats")?;
    let first = funded
        .first()
        .ok_or_else(|| RuleError("No funded seats".into()))?;
    let dealer = funded
        .iter()
        .find(|p| p.1 > state.dealer.unwrap_or_default())
        .unwrap_or(first)
        .clone();
    state.hand_no = state
        .hand_no
        .checked_add(1)
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| RuleError("Hand limit reached".into()))?;
    state.dealer = Some(dealer.1);
    let ids = funded.into_iter().map(|p| p.0).collect::<Vec<_>>();
    let zero = ids
        .iter()
        .map(|id| (id.clone(), 0))
        .collect::<BTreeMap<_, _>>();
    state.hand = Some(Hand {
        ids: ids.clone(),
        street: Street::Preflop,
        turn: None,
        folded: Vec::new(),
        acted: BTreeMap::new(),
        bets: zero.clone(),
        contributions: zero,
        current: state.big,
        last_raise: state.big,
        awards: Vec::new(),
        sb: None,
        bb: None,
    });
    state.phase = Phase::Playing;
    let order = clockwise(state, &ids, dealer.1)?;
    let sb = if ids.len() == 2 {
        dealer.0
    } else {
        order
            .first()
            .cloned()
            .ok_or_else(|| RuleError("Missing small blind".into()))?
    };
    let bb = order
        .get(if ids.len() == 2 { 0 } else { 1 })
        .cloned()
        .ok_or_else(|| RuleError("Missing big blind".into()))?;
    pay(state, &sb, state.small)?;
    pay(state, &bb, state.big)?;
    let after = player(state, &bb)?
        .seat
        .ok_or_else(|| RuleError("Big blind has no seat".into()))?;
    let h = hand_mut(state)?;
    h.sb = Some(sb);
    h.bb = Some(bb);
    audit(
        state,
        format!(
            "Hand {} started · blinds {}/{}",
            state.hand_no, state.small, state.big
        ),
    );
    progress(state, after)
}

fn bet(
    state: &mut GameState,
    actor: &str,
    action: BetAction,
    value: Option<u64>,
    target_player: Option<&str>,
    takeover: bool,
) -> Result<()> {
    ensure(state.phase == Phase::Playing, "No betting round")?;
    let id = target_player
        .filter(|id| *id != actor)
        .unwrap_or(actor)
        .to_owned();
    if id != actor {
        require_host(state, actor)?;
        ensure(takeover, "Explicit host control required")?;
        let display = player(state, &id)?.name.clone();
        audit(state, format!("Host controls {display}"));
    }
    let rules = legal(state, &id).ok_or_else(|| RuleError("Wait for your turn".into()))?;
    match action {
        BetAction::Fold => hand_mut(state)?.folded.push(id.clone()),
        BetAction::Check => ensure(rules.check, "Must call or fold")?,
        BetAction::Call => {
            ensure(!rules.check, "Nothing to call")?;
            pay(state, &id, rules.to_call)?;
        }
        BetAction::Raise | BetAction::Allin => {
            let target = if action == BetAction::Allin {
                rules.max_raise
            } else {
                value.ok_or_else(|| RuleError("Invalid raise total".into()))?
            };
            let current = hand(state)?.current;
            let bet = hand(state)?.bets.get(&id).copied().unwrap_or_default();
            ensure(
                target <= MAX_SAFE_INTEGER && target > bet && target <= rules.max_raise,
                "Invalid raise total",
            )?;
            if target <= current {
                ensure(target == rules.max_raise, "Use call")?;
                pay(state, &id, target - bet)?;
            } else {
                ensure(rules.can_raise, "Betting has not reopened")?;
                ensure(
                    target >= rules.min_raise || target == rules.max_raise,
                    format!("Minimum raise to {}", rules.min_raise),
                )?;
                let increase = target - current;
                pay(state, &id, target - bet)?;
                let h = hand_mut(state)?;
                if increase >= h.last_raise {
                    h.last_raise = increase;
                }
                h.current = target;
            }
        }
    }
    let h = hand_mut(state)?;
    h.acted.insert(id.clone(), h.current);
    let p = player(state, &id)?;
    let display = p.name.clone();
    let seat = p
        .seat
        .ok_or_else(|| RuleError("Player has no seat".into()))?;
    let suffix = if action == BetAction::Raise {
        format!(" to {}", value.unwrap_or_default())
    } else {
        String::new()
    };
    audit(state, format!("{display}: {}{suffix}", action.label()));
    progress(state, seat)
}

fn award(state: &mut GameState, winners: &[Vec<String>]) -> Result<()> {
    ensure(state.phase == Phase::Showdown, "No showdown")?;
    let all = pots(state);
    ensure(winners.len() == all.len(), "Choose winners for every pot")?;
    let dealer = state
        .dealer
        .ok_or_else(|| RuleError("Missing dealer".into()))?;
    hand_mut(state)?.awards.clear();
    for (i, (pot, wins)) in all.iter().zip(winners).enumerate() {
        ensure(
            !wins.is_empty()
                && wins.iter().collect::<BTreeSet<_>>().len() == wins.len()
                && wins.iter().all(|id| pot.eligible.contains(id)),
            "Invalid pot winners",
        )?;
        let count = u64::try_from(wins.len()).map_err(|_| RuleError("Too many winners".into()))?;
        let mut odd = pot.amount % count;
        for id in &clockwise(state, wins, dealer)? {
            let value = pot.amount / count + u64::from(odd > 0);
            odd = odd.saturating_sub(1);
            let p = player_mut(state, id)?;
            p.stack += value;
            let display = p.name.clone();
            hand_mut(state)?.awards.push(Award {
                player_id: id.clone(),
                amount: value,
            });
            audit(state, format!("{display} wins {value} from pot {}", i + 1));
        }
    }
    hand_mut(state)?.contributions.clear();
    finish(state)
}

pub fn balances(state: &GameState) -> Result<Vec<Balance>> {
    state
        .players
        .iter()
        .map(|p| {
            let sum = |kind| {
                state
                    .ledger
                    .iter()
                    .filter(|e| e.player_id == p.id && e.kind == kind)
                    .try_fold(0u64, |sum, e| sum.checked_add(e.amount))
                    .ok_or_else(|| RuleError("Ledger amount overflow".into()))
            };
            let buyin = sum(CashKind::Buyin)?;
            let cashout = sum(CashKind::Cashout)?;
            ensure(
                buyin <= MAX_SAFE_INTEGER && cashout <= MAX_SAFE_INTEGER - p.stack,
                "Ledger amount limit exceeded",
            )?;
            let net = i64::try_from(cashout + p.stack)
                .map_err(|_| RuleError("Ledger amount overflow".into()))?
                - i64::try_from(buyin).map_err(|_| RuleError("Ledger amount overflow".into()))?;
            Ok(Balance {
                player_id: p.id.clone(),
                name: p.name.clone(),
                buyin,
                cashout,
                stack: p.stack,
                net,
            })
        })
        .collect()
}

pub fn settlement(state: &GameState) -> Result<Vec<Settlement>> {
    let all = balances(state)?;
    let mut debt = all
        .iter()
        .filter(|b| b.net < 0)
        .map(|b| (b.player_id.clone(), b.net.unsigned_abs()))
        .collect::<Vec<_>>();
    let mut credit = all
        .iter()
        .filter(|b| b.net > 0)
        .map(|b| (b.player_id.clone(), b.net.unsigned_abs()))
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for (from, owed) in &mut debt {
        for (to, receives) in &mut credit {
            let value = (*owed).min(*receives);
            if value > 0 {
                result.push(Settlement {
                    from: from.clone(),
                    to: to.clone(),
                    amount: value,
                });
                *owed -= value;
                *receives -= value;
            }
        }
    }
    Ok(result)
}
