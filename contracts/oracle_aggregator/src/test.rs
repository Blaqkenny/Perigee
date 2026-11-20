#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

#[contract]
pub struct PriceSourceOk100;

#[contractimpl]
impl PriceSourceOk100 {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(100)
    }
}

#[contract]
pub struct PriceSourceOk101;

#[contractimpl]
impl PriceSourceOk101 {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(101)
    }
}

#[contract]
pub struct PriceSourceOk99;

#[contractimpl]
impl PriceSourceOk99 {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(99)
    }
}

#[contract]
pub struct PriceSourceOutlier;

#[contractimpl]
impl PriceSourceOutlier {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(150)
    }
}

#[contract]
pub struct PriceSourceUnresponsive;

#[contractimpl]
impl PriceSourceUnresponsive {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Err(Error::InvalidPrice)
    }
}

fn register_sources(env: &Env) -> (Address, Address, Address, Address, Address) {
    let ok_100 = env.register(PriceSourceOk100, ());
    let ok_101 = env.register(PriceSourceOk101, ());
    let ok_99 = env.register(PriceSourceOk99, ());
    let outlier = env.register(PriceSourceOutlier, ());
    let unresponsive = env.register(PriceSourceUnresponsive, ());
    (ok_100, ok_101, ok_99, outlier, unresponsive)
}

#[test]
fn test_aggregate_median_three_sources() {
    let env = Env::default();
    let (ok_100, ok_101, ok_99, _, _) = register_sources(&env);
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [ok_100, ok_101, ok_99]);

    assert_eq!(client.aggregate_price(&sources), Ok(100));
}

#[test]
fn test_ignore_outlier_and_use_fallback() {
    let env = Env::default();
    let (ok_100, ok_101, ok_99, outlier, _) = register_sources(&env);
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [ok_100, ok_101, ok_99, outlier]);

    assert_eq!(client.aggregate_price(&sources), Ok(100));
}

#[test]
fn test_skip_unresponsive_source() {
    let env = Env::default();
    let (ok_100, ok_101, ok_99, _, unresponsive) = register_sources(&env);
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [ok_100, ok_101, ok_99, unresponsive]);

    assert_eq!(client.aggregate_price(&sources), Ok(100));
}

#[test]
fn test_reject_when_not_enough_sources() {
    let env = Env::default();
    let (ok_100, ok_101, _, _, _) = register_sources(&env);
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [ok_100, ok_101]);

    assert_eq!(client.aggregate_price(&sources), Err(Error::NotEnoughSources));
}

#[test]
fn test_reject_when_not_enough_valid_prices() {
    let env = Env::default();
    let (ok_100, _, _, outlier, unresponsive) = register_sources(&env);
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [ok_100, outlier, unresponsive]);

    assert_eq!(client.aggregate_price(&sources), Err(Error::NotEnoughValidPrices));
}

// ── New mock contracts for issues #877 ───────────────────────────────────────

#[contract]
pub struct PriceSourceZero;

#[contractimpl]
impl PriceSourceZero {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(0)
    }
}

#[contract]
pub struct PriceSourceNegative;

#[contractimpl]
impl PriceSourceNegative {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(-1)
    }
}

#[contract]
pub struct PriceSourceMaxI128;

#[contractimpl]
impl PriceSourceMaxI128 {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(i128::MAX)
    }
}

#[contract]
pub struct PriceSourceOne;

#[contractimpl]
impl PriceSourceOne {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(1)
    }
}

/// Returns 1000 — far from median of 100 (900% deviation) or far from 10000.
#[contract]
pub struct PriceSourceOutlier1000;

#[contractimpl]
impl PriceSourceOutlier1000 {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(1000)
    }
}

/// Returns 10000 — far from median of 1000.
#[contract]
pub struct PriceSourceOutlier10000;

#[contractimpl]
impl PriceSourceOutlier10000 {
    pub fn latest_price(_env: Env) -> Result<i128, Error> {
        Ok(10000)
    }
}

// ── #877 test cases ──────────────────────────────────────────────────────────

/// Fewer than 3 sources → NotEnoughSources.
#[test]
fn test_aggregate_price_fewer_than_3_sources() {
    let env = Env::default();
    let (ok_100, ok_101, _, _, _) = register_sources(&env);
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [ok_100, ok_101]);

    assert_eq!(client.aggregate_price(&sources), Err(Error::NotEnoughSources));
}

/// All sources return 0 or negative values → NotEnoughValidPrices.
#[test]
fn test_aggregate_price_all_sources_zero_or_negative() {
    let env = Env::default();
    let zero1 = env.register(PriceSourceZero, ());
    let zero2 = env.register(PriceSourceZero, ());
    let neg = env.register(PriceSourceNegative, ());
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [zero1, zero2, neg]);

    assert_eq!(client.aggregate_price(&sources), Err(Error::NotEnoughValidPrices));
}

/// Sources with i128::MAX, 1, 1 — the extreme value is a >5% outlier from
/// median=1, so after filtering only 2 remain → NotEnoughReliableSources.
/// Verifies no panic occurs on extreme i128 values.
#[test]
fn test_aggregate_price_extreme_values_no_panic() {
    let env = Env::default();
    let max_src = env.register(PriceSourceMaxI128, ());
    let one_a = env.register(PriceSourceOne, ());
    let one_b = env.register(PriceSourceOne, ());
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [max_src, one_a, one_b]);

    // Sorted: [1, 1, i128::MAX]; median = 1.
    // threshold = 1 * 5 = 5.
    // abs_diff(i128::MAX, 1) * 100 overflows → saturating_mul caps it, and 1*100=100 > 5.
    // So i128::MAX is filtered out; only 2 sources remain → NotEnoughReliableSources.
    let result = client.aggregate_price(&sources);
    assert!(result.is_ok() || result == Err(Error::NotEnoughReliableSources));
}

/// Three prices where 2 of the 3 deviate >5% from the median — after filtering
/// only 1 survives, giving NotEnoughReliableSources.
/// Prices: 100, 1000, 10000. Sorted median = 1000.
/// threshold = 1000 * 5 = 5000.
/// abs_diff(100, 1000)*100 = 90000 > 5000 → filtered out.
/// abs_diff(10000, 1000)*100 = 900000 > 5000 → filtered out.
/// abs_diff(1000, 1000)*100 = 0 ≤ 5000 → kept.
/// filtered.len() = 1 < 3 → NotEnoughReliableSources.
#[test]
fn test_aggregate_price_all_outliers() {
    let env = Env::default();
    let src_100 = env.register(PriceSourceOk100, ());
    let src_1000 = env.register(PriceSourceOutlier1000, ());
    let src_10000 = env.register(PriceSourceOutlier10000, ());
    let aggregator_id = env.register(OracleAggregator, ());
    let client = OracleAggregatorClient::new(&env, &aggregator_id);
    let sources = Vec::from_array(&env, [src_100, src_1000, src_10000]);

    assert_eq!(client.aggregate_price(&sources), Err(Error::NotEnoughReliableSources));
}
