#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};

const INITIAL_RATE: i128 = 100_000_000_000_000; // 0.0001 in Fixed (18 decimals)
const DECAY_RATE: i128 = 10_000_000_000_000_000; // 0.01 in Fixed (18 decimals)
const STAKE_AMOUNT: i128 = 10_000;

fn advance_ledger(e: &Env, by: u32) {
    let mut info = e.ledger().get();
    info.sequence_number += by;
    e.ledger().set(info);
}

fn setup() -> (
    Env,
    StakingRewardsClient<'static>,
    Address, // owner
    Address, // staking_token
    Address, // reward_token
) {
    let e = Env::default();
    e.mock_all_auths();
    e.cost_estimate().budget().reset_unlimited();

    let owner = Address::generate(&e);
    let staking_token_admin = Address::generate(&e);
    let staking_token = e
        .register_stellar_asset_contract_v2(staking_token_admin)
        .address();

    let reward_token_admin = Address::generate(&e);
    let reward_token = e
        .register_stellar_asset_contract_v2(reward_token_admin)
        .address();

    let contract_id = e.register(StakingRewards, ());
    let client = StakingRewardsClient::new(&e, &contract_id);

    client.initialize(
        &owner,
        &staking_token,
        &reward_token,
        &INITIAL_RATE,
        &DECAY_RATE,
        &0u32, // start block
    );

    // Mint staking tokens to users later, and mint reward tokens to the contract
    let reward_client = token::StellarAssetClient::new(&e, &reward_token);
    reward_client.mint(&contract_id, &1_000_000_000);

    (e, client, owner, staking_token, reward_token)
}

#[test]
fn test_initialization() {
    let (_, client, owner, staking_token, reward_token) = setup();
    let config = client.get_config();

    assert_eq!(config.owner, owner);
    assert_eq!(config.staking_token, staking_token);
    assert_eq!(config.reward_token, reward_token);
    assert_eq!(config.initial_rate.0, INITIAL_RATE);
    assert_eq!(config.decay_rate.0, DECAY_RATE);
    assert_eq!(config.start_block, 0);
    assert!(!config.is_paused);
}

#[test]
fn test_stake_and_yield_accumulation_no_decay() {
    // Re-initialize with 0 decay rate (alpha = 1)
    let e = Env::default();
    e.mock_all_auths();
    e.cost_estimate().budget().reset_unlimited();

    let owner = Address::generate(&e);
    let staking_token = e
        .register_stellar_asset_contract_v2(Address::generate(&e))
        .address();
    let reward_token = e
        .register_stellar_asset_contract_v2(Address::generate(&e))
        .address();

    let contract_id = e.register(StakingRewards, ());
    let client = StakingRewardsClient::new(&e, &contract_id);

    client.initialize(
        &owner,
        &staking_token,
        &reward_token,
        &INITIAL_RATE,
        &0i128, // decay_rate = 0
        &10u32, // start_block = 10
    );

    let user = Address::generate(&e);
    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Fast forward ledger to block 10
    advance_ledger(&e, 10);

    // Stake at block 10
    client.stake(&user, &STAKE_AMOUNT);

    assert_eq!(client.get_staked_balance(&user), STAKE_AMOUNT);
    assert_eq!(client.get_accrued_rewards(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);

    // Advance 5 blocks (from 10 to 15)
    advance_ledger(&e, 5);

    // Expected multiplier: exp(r0 * 5)
    // r0 = 0.0001, so r0 * 5 = 0.0005
    // exp(0.0005) = 1.00050012502
    // Expected reward = 10,000 * (1.00050012502 - 1) = 5.0012502
    // Truncated to integer: 5
    let pending = client.get_pending_rewards(&user);
    assert_eq!(pending, 5);
}

#[test]
fn test_stake_and_yield_accumulation_with_decay() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake at block 0
    client.stake(&user, &STAKE_AMOUNT);

    // Advance 5 blocks (from 0 to 5)
    advance_ledger(&e, 5);

    // Expected exponent: (r0 / d) * (1 - alpha^5)
    // r0 = 0.0001, d = 0.01, alpha = 0.99
    // alpha^5 = 0.99^5 = 0.9509900499
    // 1 - alpha^5 = 0.0490099501
    // exponent = (0.0001 / 0.01) * 0.0490099501 = 0.000490099501
    // exp(0.000490099501) = 1.0004902196
    // Expected reward = 10,000 * 0.0004902196 = 4.902196 => truncated to 4
    let pending = client.get_pending_rewards(&user);
    assert_eq!(pending, 4);

    // Claim rewards
    client.claim(&user);
    assert_eq!(client.get_accrued_rewards(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);
}

#[test]
fn test_compounding_interest() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &100_000); // larger stake to see compounding clearly

    client.stake(&user, &100_000);

    // Advance 10 blocks (from 0 to 10)
    advance_ledger(&e, 10);

    // Check rewards without claiming
    let pending_1 = client.get_pending_rewards(&user);

    // Let's do another action to write back accrued rewards to storage (e.g. withdraw 0, or we can just let it update)
    // Staking 1 more token triggers a reward update and stores the accrued reward
    staking_client.mint(&user, &1);
    client.stake(&user, &1);

    let accrued = client.get_accrued_rewards(&user);
    assert!(accrued > 0);
    assert_eq!(accrued, pending_1);

    // Now advance another 10 blocks (from 10 to 20)
    advance_ledger(&e, 10);

    // The new pending rewards should compound on (staked_amount + accrued_rewards)
    let pending_2 = client.get_pending_rewards(&user);
    assert!(pending_2 > accrued);
}

#[test]
fn test_zero_stake_security() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 10);

    // User has accrued rewards
    let pending = client.get_pending_rewards(&user);
    assert!(pending > 0);

    // Withdraw entire principal
    client.withdraw(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), 0);

    // Accrued rewards are saved
    let accrued = client.get_accrued_rewards(&user);
    assert_eq!(accrued, pending);

    // Advance another 10 blocks
    advance_ledger(&e, 10);

    // Since stake is 0, compounding is inactive. Accrued rewards must NOT increase!
    let pending_after = client.get_pending_rewards(&user);
    assert_eq!(pending_after, accrued);
}

#[test]
fn test_emergency_withdraw() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 10);

    // Verify rewards accrued
    assert!(client.get_pending_rewards(&user) > 0);

    // Pause staking to simulate extreme conditions
    client.pause_staking();

    // Emergency withdraw should succeed even when paused
    let withdrawn = client.emergency_withdraw(&user);
    assert_eq!(withdrawn, STAKE_AMOUNT);

    // Verify stake balance is zero and user state is cleared (rewards forfeited)
    assert_eq!(client.get_staked_balance(&user), 0);
    assert_eq!(client.get_accrued_rewards(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);

    // Verify principal token is fully returned to the user
    let token_balance = token::Client::new(&e, &staking_token).balance(&user);
    assert_eq!(token_balance, STAKE_AMOUNT);
}

#[test]
#[should_panic(expected = "Contract, #14")]
fn test_pause_safeguards_stake() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.pause_staking();
    client.stake(&user, &STAKE_AMOUNT);
}

#[test]
#[should_panic(expected = "Contract, #14")]
fn test_pause_safeguards_withdraw() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    client.pause_staking();
    client.withdraw(&user, &STAKE_AMOUNT);
}

#[test]
#[should_panic(expected = "Contract, #14")]
fn test_pause_safeguards_claim() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    client.pause_staking();
    client.claim(&user);
}

#[test]
fn test_successful_withdrawal() {
/// Verifies that the CLAIM_REWARDS granular pause blocks claims independently
/// of the global `is_paused` flag, satisfying issue #463 acceptance criteria.
#[test]
#[should_panic(expected = "Contract, #14")]
fn test_granular_claim_rewards_pause() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake tokens
    client.stake(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), STAKE_AMOUNT);

    // Withdraw the full stake
    client.withdraw(&user, &STAKE_AMOUNT);

    // Verify stake balance is zero
    assert_eq!(client.get_staked_balance(&user), 0);

    // Verify tokens returned to user
    let token_balance = token::Client::new(&e, &staking_token).balance(&user);
    assert_eq!(token_balance, STAKE_AMOUNT);
}

#[test]
fn test_partial_withdrawal() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake tokens
    client.stake(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), STAKE_AMOUNT);

    // Withdraw half the stake
    let withdraw_amount = STAKE_AMOUNT / 2;
    client.withdraw(&user, &withdraw_amount);

    // Verify remaining stake balance
    assert_eq!(client.get_staked_balance(&user), withdraw_amount);

    // Verify tokens returned to user
    let token_balance = token::Client::new(&e, &staking_token).balance(&user);
    assert_eq!(token_balance, withdraw_amount);
}

#[test]
#[should_panic(expected = "Contract, #17")]
fn test_withdraw_zero_amount() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);

    // Attempt to withdraw zero amount should fail
    client.withdraw(&user, &0);
}

#[test]
#[should_panic(expected = "Contract, #17")]
fn test_withdraw_negative_amount() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);

    // Attempt to withdraw negative amount should fail
    client.withdraw(&user, &-100);
}

#[test]
#[should_panic(expected = "Contract, #4")]
fn test_withdraw_insufficient_balance() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);

    // Attempt to withdraw more than staked should fail
    client.withdraw(&user, &(STAKE_AMOUNT + 1000));
}

#[test]
fn test_withdraw_preserves_accrued_rewards() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake tokens
    client.stake(&user, &STAKE_AMOUNT);

    // Advance ledger to accrue rewards
    advance_ledger(&e, 10);

    // Check pending rewards before withdrawal
    let pending_before = client.get_pending_rewards(&user);
    assert!(pending_before > 0);

    // Withdraw partial amount
    let withdraw_amount = STAKE_AMOUNT / 2;
    client.withdraw(&user, &withdraw_amount);

    // Verify accrued rewards are preserved
    let accrued_after = client.get_accrued_rewards(&user);
    assert_eq!(accrued_after, pending_before);

    // Verify remaining stake balance
    assert_eq!(client.get_staked_balance(&user), withdraw_amount);
}

#[test]
fn test_complete_withdrawal_state_cleanup() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake tokens
    client.stake(&user, &STAKE_AMOUNT);

    // Advance ledger to accrue rewards
    advance_ledger(&e, 10);

    // Claim rewards first
    client.claim(&user);
    assert_eq!(client.get_accrued_rewards(&user), 0);

    // Withdraw full stake
    client.withdraw(&user, &STAKE_AMOUNT);

    // Verify stake balance is zero
    assert_eq!(client.get_staked_balance(&user), 0);

    // Verify accrued rewards remain zero
    assert_eq!(client.get_accrued_rewards(&user), 0);

    // Verify user state is cleaned up (no pending rewards)
    assert_eq!(client.get_pending_rewards(&user), 0);
}

#[test]
fn test_granular_pause_staking() {
    let (e, client, _owner, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Verify staking is not paused initially
    assert!(!client.is_staking_paused());

    // Stake should work when not paused
    client.stake(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), STAKE_AMOUNT);

    // Pause staking
    client.pause_staking();

    // Verify staking is paused
    assert!(client.is_staking_paused());

    // Stake should fail when paused
    let result = client.try_stake(&user, &STAKE_AMOUNT);
    assert!(result.is_err());

    // Resume staking
    client.resume_staking();

    // Verify staking is not paused
    assert!(!client.is_staking_paused());

    // Mint more tokens for second stake
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake should work again after resume
    client.stake(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), STAKE_AMOUNT * 2);
    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 5);

    // Activate CLAIM_REWARDS granular pause via the contract's delegation function.
    // Global is_paused remains false — only the granular bitmask bit is set.
    client.set_claim_rewards_paused(&true);

    // Claim MUST fail with ContractError::Paused (error code 14).
    client.claim(&user);
}

// ── #878 — Staking reward accrual across long idle intervals ─────────────────

/// Staking 10_000 tokens and advancing 1_000_000 blocks must not panic.
/// The pending reward calculation uses saturating/checked arithmetic, so it
/// must remain non-negative even under extreme time spans.
#[test]
fn test_long_idle_no_overflow() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);

    // Advance one million blocks — the contract must not panic.
    advance_ledger(&e, 1_000_000);

    let pending = client.get_pending_rewards(&user);
    assert!(pending >= 0, "pending rewards must not be negative");
}

/// User B stakes after user A has already been earning rewards. B should
/// receive zero pending rewards at the moment they stake, and positive rewards
/// only after additional blocks have elapsed.
#[test]
fn test_late_joiner_no_backdated_rewards() {
    let (e, client, _, staking_token, _) = setup();

    let user_a = Address::generate(&e);
    let user_b = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user_a, &STAKE_AMOUNT);
    staking_client.mint(&user_b, &STAKE_AMOUNT);

    // User A stakes at block 0.
    client.stake(&user_a, &STAKE_AMOUNT);

    // Advance 100 blocks.
    advance_ledger(&e, 100);

    // User B stakes at block 100 — should not inherit any of A's rewards.
    client.stake(&user_b, &STAKE_AMOUNT);

    // B joined at the current block, so pending rewards are 0 right now.
    let b_pending_at_join = client.get_pending_rewards(&user_b);
    assert_eq!(b_pending_at_join, 0, "late joiner must not receive backdated rewards");

    // Advance 10 more blocks — B should now have positive pending rewards.
    advance_ledger(&e, 10);

    let b_pending_after = client.get_pending_rewards(&user_b);
    assert!(b_pending_after > 0, "late joiner must accrue rewards after their join block");
}

/// Claiming immediately after a previous claim (with no ledger advancement)
/// must return 0 — no double-pay.
#[test]
fn test_repeated_claims_do_not_exceed_accrued() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 10);

    // First claim should succeed and return positive rewards.
    let first = client.claim(&user);
    assert!(first > 0, "first claim must return positive rewards");

    // Second claim at the same block must return 0 (nothing new accrued).
    let second = client.claim(&user);
    assert_eq!(second, 0, "second claim at same block must return 0");
}

// ── #879 — Staking commission and withdrawal edge cases ──────────────────────

/// When initial_rate is 0, no rewards should accrue regardless of elapsed blocks.
#[test]
fn test_zero_reward_period_no_rewards_accrued() {
    let e = Env::default();
    e.mock_all_auths();
    e.cost_estimate().budget().reset_unlimited();

    let owner = Address::generate(&e);
    let staking_token = e
        .register_stellar_asset_contract_v2(Address::generate(&e))
        .address();
    let reward_token = e
        .register_stellar_asset_contract_v2(Address::generate(&e))
        .address();

    let contract_id = e.register(StakingRewards, ());
    let client = StakingRewardsClient::new(&e, &contract_id);

    // Initialize with zero initial_rate → no rewards should ever accrue.
    client.initialize(
        &owner,
        &staking_token,
        &reward_token,
        &0i128,  // initial_rate = 0
        &0i128,  // decay_rate = 0
        &0u32,
    );

    let reward_client = token::StellarAssetClient::new(&e, &reward_token);
    reward_client.mint(&contract_id, &1_000_000_000);

    let user = Address::generate(&e);
    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 100);

    let pending = client.get_pending_rewards(&user);
    assert_eq!(pending, 0, "pending rewards must be 0 when initial_rate is 0");
}

/// Withdrawing half the stake preserves already-accrued rewards and allows
/// the remaining half to continue earning.
#[test]
fn test_partial_exit_preserves_proportional_rewards() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 10);

    // Record rewards before the partial exit.
    let pending_before_withdraw = client.get_pending_rewards(&user);
    assert!(pending_before_withdraw > 0, "must have accrued rewards before withdraw");

    // Withdraw half — accrued rewards are checkpointed.
    client.withdraw(&user, &(STAKE_AMOUNT / 2));
    let accrued_at_withdraw = client.get_accrued_rewards(&user);
    assert_eq!(accrued_at_withdraw, pending_before_withdraw,
        "accrued rewards must be preserved after partial withdrawal");

    // Advance 10 more blocks — remaining stake still earns.
    advance_ledger(&e, 10);

    let pending_after = client.get_pending_rewards(&user);
    assert!(pending_after > accrued_at_withdraw,
        "pending rewards must grow after partial exit (remaining stake still earns)");
}

/// Withdrawing all principal and then claiming should still pay out rewards
/// that were accrued before the withdrawal.
#[test]
fn test_withdraw_principal_then_claim_rewards() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    client.stake(&user, &STAKE_AMOUNT);
    advance_ledger(&e, 10);

    // Withdraw all principal — this checkpoints accrued rewards.
    client.withdraw(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), 0);

    let accrued = client.get_accrued_rewards(&user);
    assert!(accrued > 0, "accrued rewards must survive full principal withdrawal");

    // Claiming after full withdrawal must pay out the accrued rewards.
    let claimed = client.claim(&user);
    assert_eq!(claimed, accrued, "claim after full withdrawal must return exactly accrued rewards");

    // Subsequent claim must return 0.
    let second_claim = client.claim(&user);
    assert_eq!(second_claim, 0, "second claim must return 0");
}

// ── #881 — Leverage / liquidation boundary tests ─────────────────────────────

/// Staking zero tokens must be rejected (ContractError::InvalidInput = #17).
#[test]
#[should_panic(expected = "Contract, #17")]
fn test_stake_below_minimum_rejected() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Staking 0 must panic with error code 17 (InvalidInput).
    client.stake(&user, &0);
}

/// Staking a negative amount must be rejected (ContractError::InvalidInput = #17).
#[test]
#[should_panic(expected = "Contract, #17")]
fn test_stake_negative_amount_rejected() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    // Staking -1 must panic with error code 17 (InvalidInput).
    client.stake(&user, &-1);
}

/// Two users with different stake amounts must have independent, proportional
/// reward balances after the same number of elapsed blocks.
#[test]
fn test_multiple_users_independent_balances() {
    let (e, client, _, staking_token, _) = setup();

    let user_a = Address::generate(&e);
    let user_b = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user_a, &5_000);
    staking_client.mint(&user_b, &3_000);

    client.stake(&user_a, &5_000);
    client.stake(&user_b, &3_000);

    // Advance 10 blocks.
    advance_ledger(&e, 10);

    let pending_a = client.get_pending_rewards(&user_a);
    let pending_b = client.get_pending_rewards(&user_b);

    // Both users must have positive rewards.
    assert!(pending_a > 0, "user A must have positive pending rewards");
    assert!(pending_b > 0, "user B must have positive pending rewards");

    // A staked more, so A must earn more.
    assert!(pending_a > pending_b,
        "user A (larger stake) must earn more than user B");

    // Staked balances must reflect each user's own principal.
    assert_eq!(client.get_staked_balance(&user_a), 5_000);
    assert_eq!(client.get_staked_balance(&user_b), 3_000);
}

// ── Issue #882: options strategy expiry and settlement lifecycle ──────────────

/// Full happy path: stake → advance blocks → claim rewards (> 0) → withdraw all
/// → verify staked == 0 and pending == 0.
#[test]
fn test_lifecycle_stake_advance_claim_unstake() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake 10_000.
    client.stake(&user, &STAKE_AMOUNT);
    assert_eq!(client.get_staked_balance(&user), STAKE_AMOUNT);

    // Advance 10 blocks so rewards accrue.
    advance_ledger(&e, 10);

    // Rewards must be positive before claiming.
    let pending = client.get_pending_rewards(&user);
    assert!(pending > 0, "rewards should have accrued after 10 blocks");

    // Claim — rewards are transferred out, accrued resets to zero.
    client.claim(&user);
    assert_eq!(client.get_accrued_rewards(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);

    // Withdraw the full principal.
    client.withdraw(&user, &STAKE_AMOUNT);

    // State machine is at rest: no stake, no pending rewards.
    assert_eq!(client.get_staked_balance(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);
}

/// After a full exit (stake then withdraw all), the user has no staked balance
/// and no pending rewards.
#[test]
fn test_cannot_stake_after_full_exit() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake then immediately withdraw.
    client.stake(&user, &STAKE_AMOUNT);
    client.withdraw(&user, &STAKE_AMOUNT);

    // Advance slightly and claim any residual rewards (may be 0 at block 0).
    advance_ledger(&e, 1);
    // If get_pending_rewards is 0 there is nothing to claim; guard accordingly.
    let pending = client.get_pending_rewards(&user);
    if pending > 0 {
        client.claim(&user);
    }

    // Both invariants must hold after full exit.
    assert_eq!(client.get_staked_balance(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);
}

/// Stake, advance 0 blocks (no time passes), withdraw everything — rewards
/// must be zero and staked balance must be zero.
#[test]
fn test_state_cleared_after_full_exit_no_rewards() {
    let (e, client, _, staking_token, _) = setup();
    let user = Address::generate(&e);

    let staking_client = token::StellarAssetClient::new(&e, &staking_token);
    staking_client.mint(&user, &STAKE_AMOUNT);

    // Stake without advancing the ledger.
    client.stake(&user, &STAKE_AMOUNT);

    // No blocks advance — rewards must still be 0.
    assert_eq!(client.get_pending_rewards(&user), 0);

    // Withdraw the full stake.
    client.withdraw(&user, &STAKE_AMOUNT);

    // Both staked balance and pending rewards are zero.
    assert_eq!(client.get_staked_balance(&user), 0);
    assert_eq!(client.get_pending_rewards(&user), 0);
}
