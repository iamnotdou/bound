//! Intentionally empty.
//!
//! `bound-integration-tests` carries no library code. Everything lives in
//! `tests/cross_contract.rs`, which instantiates all five Bound contracts in a
//! single `soroban_sdk::Env` and drives the real end-to-end flows offline.
