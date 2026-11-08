use std::{collections::BTreeMap, error::Error};

use poker_bank::{
    engine::{self, RuleError},
    model::{
        Award, BetAction, Command, CreateRequest, Currency, GameState, Hand, Phase, Player,
        PlayerStatus, Pot, Street,
    },
};

type TestResult = Result<(), Box<dyn Error>>;
fn table(stacks: &[u64]) -> engine::Result<GameState> {
    let request = CreateRequest {
        name: "Alice".into(),
        title: "Home game".into(),
        currency: Currency::Inr,
        minor_per_chip: 100,
        default_buyin: 1000,
    };
    let mut state = engine::fresh("p1".into(), &request)?;
    for i in 1..stacks.len() {
        state.players.push(Player {
            id: format!("p{}", i + 1),
            name: format!("Player {}", i + 1),
            status: PlayerStatus::Pending,
            seat: None,
            stack: 0,
        });
    }
    for (i, amount) in stacks.iter().enumerate() {
        state = host(
            &state,
            Command::Approve {
                player_id: format!("p{}", i + 1),
                amount: *amount,
            },
        )?;
    }
    Ok(state)
}
fn host(state: &GameState, command: Command) -> engine::Result<GameState> {
    engine::apply(state, "p1", &command)
}
fn act(state: &GameState, action: BetAction, amount: Option<u64>) -> engine::Result<GameState> {
    let actor = state
        .hand
        .as_ref()
        .and_then(|h| h.turn.as_deref())
        .ok_or_else(|| RuleError("No current player".into()))?;
    engine::apply(
        state,
        actor,
        &Command::Act {
            action,
            amount,
            player_id: None,
            takeover: false,
        },
    )
}
fn current(state: &GameState) -> Result<&Hand, Box<dyn Error>> {
    state.hand.as_ref().ok_or_else(|| "No hand".into())
}
fn total(state: &GameState) -> u64 {
    state.players.iter().map(|p| p.stack).sum::<u64>()
        + if matches!(state.phase, Phase::Between | Phase::Ended) {
            0
        } else {
            state
                .hand
                .as_ref()
                .map_or(0, |h| h.contributions.values().sum())
        }
}
fn turn_rules(state: &GameState) -> Result<poker_bank::model::LegalActions, Box<dyn Error>> {
    let id = current(state)?.turn.as_deref().ok_or("No turn")?;
    engine::legal(state, id).ok_or_else(|| "No legal actions".into())
}

#[test]
fn heads_up_dealer_small_blind_first_preflop_big_blind_first_postflop() -> TestResult {
    let mut s = host(&table(&[100, 100])?, Command::Start)?;
    assert_eq!(current(&s)?.sb.as_deref(), Some("p1"));
    assert_eq!(current(&s)?.bb.as_deref(), Some("p2"));
    assert_eq!(current(&s)?.turn.as_deref(), Some("p1"));
    s = act(&s, BetAction::Call, None)?;
    s = act(&s, BetAction::Check, None)?;
    assert_eq!(s.phase, Phase::Deal);
    s = host(&s, Command::Advance)?;
    assert_eq!(current(&s)?.turn.as_deref(), Some("p2"));
    Ok(())
}
#[test]
fn three_handed_order_and_next_hand_dealer_rotation() -> TestResult {
    let mut s = host(&table(&[100, 100, 100])?, Command::Start)?;
    assert_eq!(current(&s)?.turn.as_deref(), Some("p1"));
    s = act(&s, BetAction::Fold, None)?;
    s = act(&s, BetAction::Fold, None)?;
    assert_eq!(s.phase, Phase::Between);
    s = host(&s, Command::Start)?;
    assert_eq!(s.dealer, Some(2));
    assert_eq!(total(&s), 300);
    Ok(())
}
#[test]
fn validates_minimum_raise_and_rejects_out_of_turn() -> TestResult {
    let s = host(&table(&[100, 100, 100])?, Command::Start)?;
    assert!(act(&s, BetAction::Raise, Some(19)).is_err());
    assert!(
        engine::apply(
            &s,
            "p2",
            &Command::Act {
                action: BetAction::Check,
                amount: None,
                player_id: None,
                takeover: false
            }
        )
        .is_err()
    );
    assert_eq!(turn_rules(&s)?.min_raise, 20);
    let s = act(&s, BetAction::Raise, Some(25))?;
    assert_eq!(turn_rules(&s)?.min_raise, 40);
    Ok(())
}
#[test]
fn short_allin_does_not_reopen_player_who_already_acted() -> TestResult {
    let mut s = host(&table(&[100, 25, 100])?, Command::Start)?;
    s = act(&s, BetAction::Raise, Some(20))?;
    s = act(&s, BetAction::Allin, None)?;
    s = act(&s, BetAction::Call, None)?;
    assert_eq!(current(&s)?.turn.as_deref(), Some("p1"));
    assert!(!turn_rules(&s)?.can_raise);
    assert!(act(&s, BetAction::Raise, Some(50)).is_err());
    s = act(&s, BetAction::Call, None)?;
    assert_eq!(s.phase, Phase::Deal);
    Ok(())
}
#[test]
fn cumulative_short_allins_reopen_at_full_raise_threshold() -> TestResult {
    let mut s = host(&table(&[100, 25, 30])?, Command::Start)?;
    s = act(&s, BetAction::Raise, Some(20))?;
    s = act(&s, BetAction::Allin, None)?;
    s = act(&s, BetAction::Allin, None)?;
    assert!(turn_rules(&s)?.can_raise);
    s = act(&s, BetAction::Call, None)?;
    assert_eq!(s.phase, Phase::Showdown);
    assert_eq!(total(&s), 155);
    Ok(())
}
#[test]
fn short_big_blind_never_demands_an_uncalled_call() -> TestResult {
    let mut s = table(&[100, 5])?;
    s = host(&s, Command::Blinds { small: 10, big: 20 })?;
    s = host(&s, Command::Start)?;
    assert_eq!(s.phase, Phase::Showdown);
    assert_eq!(
        current(&s)?.contributions,
        BTreeMap::from([("p1".into(), 5), ("p2".into(), 5)])
    );
    assert_eq!(s.players[0].stack, 95);
    assert_eq!(total(&s), 105);
    Ok(())
}
#[test]
fn uncalled_allin_overbet_is_refunded_before_pots() -> TestResult {
    let mut s = host(&table(&[100, 30])?, Command::Start)?;
    s = act(&s, BetAction::Allin, None)?;
    s = act(&s, BetAction::Allin, None)?;
    assert_eq!(s.phase, Phase::Showdown);
    assert_eq!(s.players[0].stack, 70);
    assert_eq!(
        engine::pots(&s),
        vec![Pot {
            amount: 60,
            eligible: vec!["p1".into(), "p2".into()]
        }]
    );
    assert_eq!(total(&s), 130);
    Ok(())
}
fn synthetic_hand(contributions: [u64; 3], folded: Vec<String>) -> Hand {
    let ids = vec!["p1".to_owned(), "p2".to_owned(), "p3".to_owned()];
    Hand {
        ids: ids.clone(),
        street: Street::River,
        turn: None,
        folded,
        acted: BTreeMap::new(),
        bets: BTreeMap::new(),
        contributions: ids.into_iter().zip(contributions).collect(),
        current: 0,
        last_raise: 10,
        awards: Vec::new(),
        sb: Some("p2".into()),
        bb: Some("p3".into()),
    }
}
#[test]
fn side_pots_include_folded_contributions_exclude_folded_winners() -> TestResult {
    let mut s = table(&[100, 100, 100])?;
    s.hand = Some(synthetic_hand([30, 60, 60], vec!["p3".into()]));
    assert_eq!(
        engine::pots(&s),
        vec![
            Pot {
                amount: 90,
                eligible: vec!["p1".into(), "p2".into()]
            },
            Pot {
                amount: 60,
                eligible: vec!["p2".into()]
            }
        ]
    );
    Ok(())
}
#[test]
fn ties_award_odd_chip_left_of_dealer_and_reject_ineligible_winner() -> TestResult {
    let mut s = table(&[100, 100, 100])?;
    s.phase = Phase::Showdown;
    s.dealer = Some(1);
    s.hand = Some(synthetic_hand([1, 1, 1], vec!["p3".into()]));
    s.players.iter_mut().for_each(|p| p.stack = 99);
    assert!(
        host(
            &s,
            Command::Award {
                winners: vec![vec!["p3".into()]]
            }
        )
        .is_err()
    );
    s = host(
        &s,
        Command::Award {
            winners: vec![vec!["p1".into(), "p2".into()]],
        },
    )?;
    assert_eq!(s.players[0].stack, 100);
    assert_eq!(s.players[1].stack, 101);
    assert_eq!(total(&s), 300);
    Ok(())
}
#[test]
fn full_hand_checks_all_streets_then_next_hand_conserves_chips() -> TestResult {
    let mut s = host(&table(&[100, 100])?, Command::Start)?;
    s = act(&s, BetAction::Call, None)?;
    s = act(&s, BetAction::Check, None)?;
    for _ in 0..3 {
        s = host(&s, Command::Advance)?;
        s = act(&s, BetAction::Check, None)?;
        s = act(&s, BetAction::Check, None)?;
    }
    assert_eq!(s.phase, Phase::Showdown);
    s = host(
        &s,
        Command::Award {
            winners: vec![vec!["p1".into()]],
        },
    )?;
    assert_eq!(s.phase, Phase::Between);
    assert_eq!(total(&s), 200);
    s = host(&s, Command::Start)?;
    assert_eq!(s.dealer, Some(2));
    Ok(())
}
#[test]
fn integer_positive_finite_amounts_only_and_cash_seat_blinds_between_hands() -> TestResult {
    let s = table(&[100, 100, 100])?;
    for n in [0, 10_000_001, u64::MAX] {
        assert!(
            host(
                &s,
                Command::Buyin {
                    player_id: "p1".into(),
                    amount: n
                }
            )
            .is_err()
        );
    }
    for invalid in [
        r#"{"type":"buyin","playerId":"p1","amount":1.5}"#,
        r#"{"type":"buyin","playerId":"p1","amount":-1}"#,
        r#"{"type":"buyin","playerId":"p1","amount":null}"#,
        r#"{"type":"buyin","playerId":"p1","amount":1e999}"#,
    ] {
        assert!(serde_json::from_str::<Command>(invalid).is_err());
    }
    let s = host(&s, Command::Start)?;
    for command in [
        Command::Buyin {
            player_id: "p1".into(),
            amount: 10,
        },
        Command::Cashout {
            player_id: "p1".into(),
            amount: 10,
        },
        Command::Seat {
            player_id: "p1".into(),
            seat: 2,
        },
        Command::Blinds { small: 5, big: 10 },
    ] {
        assert!(host(&s, command).is_err());
    }
    Ok(())
}
#[test]
fn undo_restores_game_and_stacks_while_cash_ledger_stays_permanent() -> TestResult {
    let before = host(&table(&[100, 100, 100])?, Command::Start)?;
    let s = act(&before, BetAction::Raise, Some(40))?;
    let s = host(&s, Command::Undo)?;
    assert_eq!(s.hand, before.hand);
    assert_eq!(s.players, before.players);
    assert_eq!(s.ledger, before.ledger);
    assert!(
        s.log
            .last()
            .is_some_and(|entry| entry.text.contains("undid"))
    );
    Ok(())
}
#[test]
fn host_authority_pause_transfer_and_explicit_takeover() -> TestResult {
    let mut s = host(&table(&[100, 100, 100])?, Command::Start)?;
    assert!(engine::apply(&s, "p2", &Command::Pause).is_err());
    s = host(&s, Command::Pause)?;
    assert!(act(&s, BetAction::Fold, None).is_err());
    s = host(&s, Command::Pause)?;
    s = act(&s, BetAction::Call, None)?;
    assert!(
        host(
            &s,
            Command::Act {
                action: BetAction::Fold,
                amount: None,
                player_id: Some("p2".into()),
                takeover: false
            }
        )
        .is_err()
    );
    s = host(
        &s,
        Command::Act {
            action: BetAction::Fold,
            amount: None,
            player_id: Some("p2".into()),
            takeover: true,
        },
    )?;
    s = host(
        &s,
        Command::Transfer {
            player_id: "p2".into(),
        },
    )?;
    assert_eq!(s.host_id, "p2");
    Ok(())
}
#[test]
fn partial_cashout_settlement_and_end_session_cashout_are_consistent() -> TestResult {
    let mut s = host(&table(&[100, 100])?, Command::Start)?;
    s = act(&s, BetAction::Fold, None)?;
    s = host(
        &s,
        Command::Cashout {
            player_id: "p2".into(),
            amount: 50,
        },
    )?;
    let b = engine::balances(&s)?;
    assert_eq!(b[0].net, -5);
    assert_eq!(b[1].net, 5);
    let transfers = engine::settlement(&s)?;
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].from, "p1");
    assert_eq!(transfers[0].to, "p2");
    assert_eq!(transfers[0].amount, 5);
    s = host(&s, Command::End)?;
    assert_eq!(s.phase, Phase::Ended);
    assert!(s.players.iter().all(|p| p.stack == 0));
    assert_eq!(s.ledger.len(), 5);
    assert!(host(&s, Command::Pause).is_err());
    Ok(())
}
#[test]
fn rejecting_pending_request_does_not_block_gameplay_undo() -> TestResult {
    let mut s = table(&[100, 100, 100])?;
    s.players.push(Player {
        id: "pending".into(),
        name: "Waiting".into(),
        status: PlayerStatus::Pending,
        seat: None,
        stack: 0,
    });
    s = host(&s, Command::Start)?;
    s = host(
        &s,
        Command::Reject {
            player_id: "pending".into(),
        },
    )?;
    s = host(&s, Command::Undo)?;
    assert_eq!(s.phase, Phase::Between);
    assert_eq!(s.players.len(), 3);
    assert_eq!(total(&s), 300);
    Ok(())
}
#[test]
fn random_legal_multiway_hands_preserve_every_chip() -> TestResult {
    let mut seed = 42u32;
    let mut random = |max: usize| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        usize::from((seed >> 16) as u16) % max
    };
    for _ in 0..100 {
        let stacks = (0..2 + random(9))
            .map(|_| 20 + random(200) as u64)
            .collect::<Vec<_>>();
        let mut s = host(&table(&stacks)?, Command::Start)?;
        let expected: u64 = stacks.iter().sum();
        let mut steps = 0;
        while s.phase != Phase::Between {
            steps += 1;
            assert!(steps < 400);
            assert_eq!(total(&s), expected);
            match s.phase {
                Phase::Deal => s = host(&s, Command::Advance)?,
                Phase::Showdown => {
                    let winners = engine::pots(&s)
                        .iter()
                        .map(|pot| vec![pot.eligible[random(pot.eligible.len())].clone()])
                        .collect();
                    s = host(&s, Command::Award { winners })?;
                }
                _ => {
                    let rules = turn_rules(&s)?;
                    let roll = random(100);
                    let action = if roll < 10 {
                        BetAction::Fold
                    } else if roll < 20
                        && (rules.can_raise || rules.max_raise <= current(&s)?.current)
                    {
                        BetAction::Allin
                    } else if rules.check {
                        BetAction::Check
                    } else {
                        BetAction::Call
                    };
                    s = act(&s, action, None)?;
                }
            }
        }
        assert_eq!(total(&s), expected);
    }
    Ok(())
}
#[test]
fn javascript_saved_showdown_roundtrips_and_can_continue_in_rust() -> TestResult {
    let fixture = include_str!("fixtures/javascript-showdown.json");
    let state: GameState = serde_json::from_str(fixture)?;
    assert_eq!(
        serde_json::to_value(&state)?,
        serde_json::from_str::<serde_json::Value>(fixture)?
    );
    assert_eq!(state.phase, Phase::Showdown);
    assert_eq!(
        engine::pots(&state)
            .iter()
            .map(|pot| pot.amount)
            .sum::<u64>(),
        250
    );
    let winners = engine::pots(&state)
        .iter()
        .map(|pot| vec![pot.eligible[0].clone()])
        .collect();
    let s = host(&state, Command::Award { winners })?;
    assert_eq!(total(&s), 250);
    assert_eq!(s.ledger, state.ledger);
    let s = host(&s, Command::Undo)?;
    assert_eq!(s.hand, state.hand);
    assert_eq!(s.players, state.players);
    Ok(())
}
#[test]
fn ten_seat_capacity_seat_swap_cashout_remove_and_total_chip_cap() -> TestResult {
    let mut s = table(&[100; 10])?;
    s.players.push(Player {
        id: "extra".into(),
        name: "Extra".into(),
        status: PlayerStatus::Pending,
        seat: None,
        stack: 0,
    });
    assert!(
        host(
            &s,
            Command::Approve {
                player_id: "extra".into(),
                amount: 100
            }
        )
        .is_err()
    );
    s = host(
        &s,
        Command::Seat {
            player_id: "p1".into(),
            seat: 2,
        },
    )?;
    assert_eq!(s.players[0].seat, Some(2));
    assert_eq!(s.players[1].seat, Some(1));
    s = host(
        &s,
        Command::Cashout {
            player_id: "p2".into(),
            amount: 100,
        },
    )?;
    s = host(
        &s,
        Command::Remove {
            player_id: "p2".into(),
        },
    )?;
    assert_eq!(s.players[1].status, PlayerStatus::Out);
    assert!(
        host(
            &s,
            Command::Buyin {
                player_id: "p1".into(),
                amount: 10_000_000
            }
        )
        .is_err()
    );
    s = host(
        &s,
        Command::Approve {
            player_id: "extra".into(),
            amount: 100,
        },
    )?;
    assert_eq!(s.players.last().and_then(|p| p.seat), Some(1));
    Ok(())
}
#[test]
fn physical_pot_awards_record_each_tied_winner_once() -> TestResult {
    let mut s = table(&[100, 100, 100])?;
    s.phase = Phase::Showdown;
    s.dealer = Some(1);
    s.hand = Some(synthetic_hand([10, 10, 10], Vec::new()));
    s.players.iter_mut().for_each(|p| p.stack = 90);
    assert!(
        host(
            &s,
            Command::Award {
                winners: vec![vec!["p1".into(), "p1".into()]]
            }
        )
        .is_err()
    );
    s = host(
        &s,
        Command::Award {
            winners: vec![vec!["p1".into(), "p2".into()]],
        },
    )?;
    assert_eq!(
        current(&s)?.awards,
        vec![
            Award {
                player_id: "p2".into(),
                amount: 15
            },
            Award {
                player_id: "p1".into(),
                amount: 15
            }
        ]
    );
    Ok(())
}
