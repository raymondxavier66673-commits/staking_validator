#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Env, Map, Symbol, symbol,
};

// ===== Data Types =====

/// Commission + metadata stored per validator.
#[contracttype]
#[derive(Clone)]
pub struct ValidatorInfo {
    /// Commission in basis points (0 - 3000 = 0% - 30%).
    pub commission_bps: u32,
}

/// A locked undelegation waiting to mature.
#[contracttype]
#[derive(Clone)]
pub struct Unbonding {
    /// Stake amount waiting to be released.
    pub amount: u64,
    /// Ledger sequence at which the unbonding completes.
    pub unlock_ledger: u32,
}

// ===== Storage Key Helpers =====

fn validators_key() -> Symbol {
    symbol!("validators")
}

fn epoch_key() -> Symbol {
    symbol!("epoch")
}

fn total_stake_key(v: &Address) -> (Symbol, Address) {
    (symbol!("tstake"), v.clone())
}

fn cumm_rps_key(v: &Address) -> (Symbol, Address) {
    (symbol!("crps"), v.clone())
}

fn stake_key(v: &Address, d: &Address) -> (Symbol, Address, Address) {
    (symbol!("stake"), v.clone(), d.clone())
}

fn debt_key(v: &Address, d: &Address) -> (Symbol, Address, Address) {
    (symbol!("debt"), v.clone(), d.clone())
}

fn reward_key(v: &Address, d: &Address) -> (Symbol, Address, Address) {
    (symbol!("rew"), v.clone(), d.clone())
}

fn unbond_key(v: &Address, d: &Address) -> (Symbol, Address, Address) {
    (symbol!("unbond"), v.clone(), d.clone())
}

/// Unbonding lock in ledgers (~1 day at 5 s / ledger close).
const LOCK_PERIOD_LEDGERS: u32 = 17_280;
/// Maximum validator commission in basis points (30%).
const MAX_COMMISSION_BPS: u32 = 3_000;
/// Scaling factor for the cumulative rewards-per-stake accumulator.
const REWARD_SCALE: u128 = 1_000_000_000;

// ===== Contract =====

/// A minimal Proof-of-Stake validator registry implemented as a single
/// Soroban contract. Validators self-register with a stake and a
/// commission; delegators delegate stake to validators; rewards are
/// accumulated per-validator and split on epoch boundaries using a
/// Synthetix-style rewards-per-stake accumulator.
#[contract]
pub struct StakingValidator;

#[contractimpl]
impl StakingValidator {
    /// Register the caller as a new PoS validator. The caller must
    /// provide a non-zero `self_stake` (counted as their own delegation)
    /// and a `commission_bps` between 0 and 3000 (i.e. 0% - 30%).
    /// Returns `true` on success.
    pub fn register_validator(
        env: Env,
        validator: Address,
        commission_bps: u32,
        self_stake: u64,
    ) -> bool {
        validator.require_auth();

        if self_stake == 0 {
            panic!("self_stake must be > 0");
        }
        if commission_bps > MAX_COMMISSION_BPS {
            panic!("commission_bps exceeds 30% cap");
        }

        let mut validators: Map<Address, ValidatorInfo> = env
            .storage()
            .instance()
            .get(&validators_key())
            .unwrap_or(Map::new(&env));

        if validators.contains_key(validator.clone()) {
            panic!("validator already registered");
        }

        validators.set(validator.clone(), ValidatorInfo { commission_bps });
        env.storage().instance().set(&validators_key(), &validators);

        // Initialize validator bookkeeping.
        env.storage()
            .instance()
            .set(&total_stake_key(&validator), &self_stake);
        env.storage()
            .instance()
            .set(&cumm_rps_key(&validator), &0u128);
        env.storage()
            .instance()
            .set(&stake_key(&validator, &validator), &self_stake);
        env.storage()
            .instance()
            .set(&debt_key(&validator, &validator), &0u128);

        true
    }

    /// Delegate `amount` of stake from `delegator` to `validator`.
    /// Returns the delegator's new total delegated amount to that
    /// validator.
    pub fn delegate(
        env: Env,
        delegator: Address,
        validator: Address,
        amount: u64,
    ) -> u64 {
        delegator.require_auth();

        if amount == 0 {
            panic!("amount must be > 0");
        }

        let validators: Map<Address, ValidatorInfo> = env
            .storage()
            .instance()
            .get(&validators_key())
            .unwrap_or(Map::new(&env));

        if !validators.contains_key(validator.clone()) {
            panic!("validator not registered");
        }

        // Settle pending rewards before stake changes.
        Self::settle_rewards(&env, &validator, &delegator);

        let s_key = stake_key(&validator, &delegator);
        let current: u64 = env.storage().instance().get(&s_key).unwrap_or(0u64);
        let new_stake = current.checked_add(amount).expect("stake overflow");
        env.storage().instance().set(&s_key, &new_stake);

        let t_key = total_stake_key(&validator);
        let total: u64 = env.storage().instance().get(&t_key).unwrap_or(0u64);
        let new_total = total.checked_add(amount).expect("total overflow");
        env.storage().instance().set(&t_key, &new_total);

        // Recompute reward debt at the new stake level.
        let cumm: u128 = env
            .storage()
            .instance()
            .get(&cumm_rps_key(&validator))
            .unwrap_or(0u128);
        env.storage()
            .instance()
            .set(&debt_key(&validator, &delegator), &(cumm * (new_stake as u128)));

        new_stake
    }

    /// Undelegate `amount` from `validator`. The amount is locked for
    /// the unbonding period and can later be withdrawn via
    /// `withdraw_unbonded`. Returns the ledger sequence at which the
    /// unbonding completes.
    pub fn undelegate(
        env: Env,
        delegator: Address,
        validator: Address,
        amount: u64,
    ) -> u32 {
        delegator.require_auth();

        if amount == 0 {
            panic!("amount must be > 0");
        }

        Self::settle_rewards(&env, &validator, &delegator);

        let s_key = stake_key(&validator, &delegator);
        let current: u64 = env.storage().instance().get(&s_key).unwrap_or(0u64);
        if amount > current {
            panic!("insufficient delegated stake");
        }
        let new_stake = current - amount;
        env.storage().instance().set(&s_key, &new_stake);

        let t_key = total_stake_key(&validator);
        let total: u64 = env.storage().instance().get(&t_key).unwrap_or(0u64);
        env.storage().instance().set(&t_key, &(total - amount));

        let cumm: u128 = env
            .storage()
            .instance()
            .get(&cumm_rps_key(&validator))
            .unwrap_or(0u128);
        env.storage()
            .instance()
            .set(&debt_key(&validator, &delegator), &(cumm * (new_stake as u128)));

        let unlock = env.ledger().sequence() + LOCK_PERIOD_LEDGERS;
        env.storage().instance().set(
            &unbond_key(&validator, &delegator),
            &Unbonding {
                amount,
                unlock_ledger: unlock,
            },
        );

        unlock
    }

    /// Withdraw a previously undelegated amount whose unbonding period
    /// has elapsed. Returns the amount withdrawn.
    pub fn withdraw_unbonded(
        env: Env,
        delegator: Address,
        validator: Address,
    ) -> u64 {
        delegator.require_auth();

        let key = unbond_key(&validator, &delegator);
        let entry: Unbonding = env
            .storage()
            .instance()
            .get(&key)
            .expect("no unbonding entry");

        if env.ledger().sequence() < entry.unlock_ledger {
            panic!("still locked");
        }

        env.storage().instance().remove(&key);
        entry.amount
    }

    /// Claim all accumulated staking rewards for a
    /// `(delegator, validator)` pair. Returns the total amount claimed.
    pub fn claim_rewards(
        env: Env,
        delegator: Address,
        validator: Address,
    ) -> u64 {
        delegator.require_auth();

        let total = Self::settle_rewards(&env, &validator, &delegator);
        env.storage()
            .instance()
            .set(&reward_key(&validator, &delegator), &0u64);
        total
    }

    /// Fund `amount` of reward tokens for a validator's pool. The
    /// cumulative rewards-per-stake for that validator is increased
    /// proportionally to the current total stake.
    pub fn fund_rewards(
        env: Env,
        funder: Address,
        validator: Address,
        amount: u64,
    ) {
        funder.require_auth();

        if amount == 0 {
            panic!("amount must be > 0");
        }

        let total: u64 = env
            .storage()
            .instance()
            .get(&total_stake_key(&validator))
            .unwrap_or(0u64);
        if total == 0 {
            panic!("validator has no stake to reward");
        }

        let cumm_key = cumm_rps_key(&validator);
        let cumm: u128 = env
            .storage()
            .instance()
            .get(&cumm_key)
            .unwrap_or(0u128);
        let delta = (amount as u128) * REWARD_SCALE / (total as u128);
        env.storage().instance().set(&cumm_key, &(cumm + delta));
    }

    /// Advance the global epoch counter. Marks an epoch boundary at
    /// which reward distributions can be reconciled.
    pub fn advance_epoch(env: Env) -> u64 {
        let cur: u64 = env
            .storage()
            .instance()
            .get(&epoch_key())
            .unwrap_or(0u64);
        let next = cur + 1;
        env.storage().instance().set(&epoch_key(), &next);
        next
    }

    /// Return the total delegated stake (self-stake + delegations)
    /// for `validator`.
    pub fn total_stake(env: Env, validator: Address) -> u64 {
        env.storage()
            .instance()
            .get(&total_stake_key(&validator))
            .unwrap_or(0u64)
    }

    /// Return the current global epoch counter.
    pub fn current_epoch(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&epoch_key())
            .unwrap_or(0u64)
    }

    /// Return a delegator's pending (unclaimed) reward for a given
    /// validator. Read-only view that does not mutate state.
    pub fn pending_rewards(
        env: Env,
        delegator: Address,
        validator: Address,
    ) -> u64 {
        let staked: u64 = env
            .storage()
            .instance()
            .get(&stake_key(&validator, &delegator))
            .unwrap_or(0u64);
        let cumm: u128 = env
            .storage()
            .instance()
            .get(&cumm_rps_key(&validator))
            .unwrap_or(0u128);
        let debt: u128 = env
            .storage()
            .instance()
            .get(&debt_key(&validator, &delegator))
            .unwrap_or(0u128);
        let owed = cumm * (staked as u128);
        let newly_accrued = owed.saturating_sub(debt) / REWARD_SCALE;
        let bucket: u64 = env
            .storage()
            .instance()
            .get(&reward_key(&validator, &delegator))
            .unwrap_or(0u64);
        bucket + (newly_accrued as u64)
    }

    // ----- internal helpers -----

    /// Recalculate and store pending rewards for
    /// `(validator, delegator)`. Returns the new total pending
    /// (bucket + newly accrued).
    fn settle_rewards(
        env: &Env,
        validator: &Address,
        delegator: &Address,
    ) -> u64 {
        let staked: u64 = env
            .storage()
            .instance()
            .get(&stake_key(validator, delegator))
            .unwrap_or(0u64);
        let cumm: u128 = env
            .storage()
            .instance()
            .get(&cumm_rps_key(validator))
            .unwrap_or(0u128);
        let debt: u128 = env
            .storage()
            .instance()
            .get(&debt_key(validator, delegator))
            .unwrap_or(0u128);

        let owed = cumm * (staked as u128);
        let new_pending = owed.saturating_sub(debt) / REWARD_SCALE;
        let new_pending_u64 = new_pending as u64;

        let r_key = reward_key(validator, delegator);
        let prev: u64 = env.storage().instance().get(&r_key).unwrap_or(0u64);
        let total = prev + new_pending_u64;
        env.storage().instance().set(&r_key, &total);
        env.storage()
            .instance()
            .set(&debt_key(validator, delegator), &owed);

        total
    }
}
