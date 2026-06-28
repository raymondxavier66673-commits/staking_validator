# staking_validator

## Project Title
staking_validator

## Project Description
Proof-of-Stake networks need a transparent on-chain registry where validators can advertise themselves and where delegators can bond their stake behind a validator of their choice. Most existing staking systems bury this logic in monolithic L1 protocols, making it hard to audit and to extend. `staking_validator` solves that by exposing the entire validator / delegator / rewards lifecycle as a single, easy-to-read Soroban smart contract on Stellar, where every state transition is verifiable and every reward split is enforced by code rather than by trust.

## Project Vision
The long-term vision is to make `staking_validator` the canonical reference implementation for a PoS validator set on Stellar: a minimal, composable, and upgradeable contract that any dApp — wallets, explorers, liquid-staking protocols, restaking services — can integrate with safely. By keeping the core simple (one contract, one storage layout) and documenting every invariant, we want to lower the barrier for builders who need staking primitives without redeploying a whole blockchain.

## Key Features
- **Validator self-registration** — `register_validator(env, validator, commission_bps, self_stake)` lets any address join the active set with a non-zero self-stake and a commission capped at 30%.
- **Delegation lifecycle** — `delegate`, `undelegate`, and `withdraw_unbonded` cover the full bond / unbond / withdraw loop, with a ledger-based unbonding lock that protects the validator from instant stake withdrawals.
- **Per-validator reward pool** — `fund_rewards` lets anyone (DAO, treasury, or external protocol) top up a validator's reward pool; the cumulative rewards-per-stake accumulator is updated proportionally to the current total stake.
- **Proportional reward claims** — `claim_rewards` uses a Synthetix-style `cumulative_rps` + per-delegator `reward_debt` model so that every claim returns exactly the amount accrued, regardless of when the delegator joined or how often rewards are topped up.
- **Read-only views** — `total_stake`, `current_epoch`, and `pending_rewards` let frontends and indexers query contract state without sending a transaction.
- **Epoch accounting** — `advance_epoch` increments a global epoch counter that off-chain consumers can use to group reward distributions into clean boundaries.

## Contract

- **Network:** Stellar Testnet (Public)
- **Scope:** finance dApp — see `contracts/staking_validator/src/lib.rs` for the full staking_validator business logic.
- **Functions exposed:** see `Key Features` above and the `pub fn` list in `lib.rs`.
- **Contract ID:** `CBFXH3G6UTXBOLODJWA2M6UQXFKWIOXC5YQ64R3CON5QWAIRL55OCNGL`
- **Explorer template:** `https://stellar.expert/explorer/testnet/tx/8a3ca7d465f3dba694f4509ea22a035949fea37320e076fcff12e2b19a1bcc44`

## Future Scope
- Integrate with a real Stellar SAC (Stellar Asset Contract) so that `self_stake`, `delegate`, `undelegate`, and `fund_rewards` move actual on-chain token balances in and out of the contract account.
- Split reward distribution at epoch boundaries so a configurable percentage of the funded pool is auto-released to delegators and the validator's commission is taken at settle-time, instead of tracking a single open-ended accumulator.
- Add slashing: a governance-controlled `slash(validator, amount, reason)` that reduces total stake and burns a portion of the validator's self-stake, with a delegator-aware accounting pass.
- Add a validator-set rotation view (`active_validators`, `top_n_by_stake`) plus a min-self-stake guard, so the contract can be used as a real consensus set rather than a permissionless registry.
- Emit Soroban events (`register`, `delegate`, `undelegate`, `claim`, `slash`) for off-chain indexers and a future React/Freighter frontend.
- Migrate per-`(validator, delegator)` state from `instance()` storage to `persistent()` / `temporary()` storage with proper TTL extension, so the contract scales beyond a handful of delegators.

## Profile

- **Name:** <!-- Fill github name -->
- **Project:** `staking_validator` (finance)
- **Built with:** Soroban SDK 25, Rust, Stellar Testnet
