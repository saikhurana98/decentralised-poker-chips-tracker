use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const MAX_CHIPS: u64 = 10_000_000;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerStatus {
    Pending,
    Active,
    Out,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Between,
    Playing,
    Deal,
    Showdown,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

impl Street {
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Preflop => Some(Self::Flop),
            Self::Flop => Some(Self::Turn),
            Self::Turn => Some(Self::River),
            Self::River => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Preflop => "preflop",
            Self::Flop => "flop",
            Self::Turn => "turn",
            Self::River => "river",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Currency {
    #[default]
    #[serde(rename = "INR")]
    Inr,
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "EUR")]
    Eur,
    #[serde(rename = "GBP")]
    Gbp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub status: PlayerStatus,
    pub seat: Option<u8>,
    pub stack: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Award {
    pub player_id: String,
    pub amount: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hand {
    pub ids: Vec<String>,
    pub street: Street,
    pub turn: Option<String>,
    pub folded: Vec<String>,
    pub acted: BTreeMap<String, u64>,
    pub bets: BTreeMap<String, u64>,
    pub contributions: BTreeMap<String, u64>,
    pub current: u64,
    pub last_raise: u64,
    pub awards: Vec<Award>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bb: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CashKind {
    Buyin,
    Cashout,
}

impl CashKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Buyin => "buyin",
            Self::Cashout => "cashout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashEntry {
    pub id: String,
    pub player_id: String,
    pub kind: CashKind,
    pub amount: u64,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub at: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub phase: Phase,
    pub hand_no: u64,
    pub dealer: Option<u8>,
    pub hand: Option<Hand>,
    pub stacks: Vec<(String, u64)>,
    #[serde(default)]
    pub log: Vec<AuditEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    pub revision: u64,
    pub host_id: String,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub currency: Currency,
    #[serde(default = "default_chip_value")]
    pub minor_per_chip: u64,
    #[serde(default = "default_buyin")]
    pub default_buyin: u64,
    pub players: Vec<Player>,
    pub small: u64,
    pub big: u64,
    pub phase: Phase,
    pub paused: bool,
    pub hand_no: u64,
    pub dealer: Option<u8>,
    pub hand: Option<Hand>,
    pub ledger: Vec<CashEntry>,
    // History is omitted only from public snapshots by moving it out before serialization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEntry>,
    pub log: Vec<AuditEntry>,
}

pub fn default_title() -> String {
    "Home game".to_owned()
}
pub const fn default_chip_value() -> u64 {
    100
}
pub const fn default_buyin() -> u64 {
    1000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BetAction {
    Fold,
    Check,
    Call,
    Raise,
    Allin,
}

impl BetAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fold => "fold",
            Self::Check => "check",
            Self::Call => "call",
            Self::Raise => "raise",
            Self::Allin => "allin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    Approve {
        player_id: String,
        amount: u64,
    },
    Reject {
        player_id: String,
    },
    Buyin {
        player_id: String,
        amount: u64,
    },
    Cashout {
        player_id: String,
        amount: u64,
    },
    Seat {
        player_id: String,
        seat: u8,
    },
    Remove {
        player_id: String,
    },
    Blinds {
        small: u64,
        big: u64,
    },
    Transfer {
        player_id: String,
    },
    Pause,
    Undo,
    Start,
    Act {
        action: BetAction,
        #[serde(default)]
        amount: Option<u64>,
        #[serde(default)]
        player_id: Option<String>,
        #[serde(default)]
        takeover: bool,
    },
    Advance,
    Award {
        winners: Vec<Vec<String>>,
    },
    End,
}

impl Command {
    pub const fn is_gameplay(&self) -> bool {
        matches!(
            self,
            Self::Start | Self::Act { .. } | Self::Advance | Self::Award { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pot {
    pub amount: u64,
    pub eligible: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegalActions {
    pub to_call: u64,
    pub check: bool,
    pub min_raise: u64,
    pub max_raise: u64,
    pub can_raise: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub player_id: String,
    pub name: String,
    pub buyin: u64,
    pub cashout: u64,
    pub stack: u64,
    pub net: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Settlement {
    pub from: String,
    pub to: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub name: String,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub currency: Currency,
    #[serde(default = "default_chip_value")]
    pub minor_per_chip: u64,
    #[serde(default = "default_buyin")]
    pub default_buyin: u64,
}

#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub name: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub code: String,
    pub token: String,
    pub player_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMessage {
    Auth {
        code: String,
        token: String,
    },
    Lobby,
    Command {
        id: String,
        revision: u64,
        command: Command,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(flatten)]
    pub game: GameState,
    pub undo_count: usize,
    pub code: String,
    pub you: String,
    pub pots: Vec<Pot>,
    pub legal: Option<LegalActions>,
    pub balances: Vec<Balance>,
    pub settlement: Vec<Settlement>,
    pub online: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LobbyTable {
    pub code: String,
    pub title: String,
    pub players: usize,
    pub phase: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerMessage {
    State {
        state: Box<Snapshot>,
    },
    Lobby {
        tables: Vec<LobbyTable>,
    },
    Ack {
        id: String,
        revision: u64,
        duplicate: bool,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        error: String,
    },
}
