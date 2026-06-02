# Bound Protocol — Verification Prompt

Paste everything below into a fresh agent session (working dir: `/Users/dogu/bound`).
It checks two things, in order: (A) does the system **work**, and (B) does it actually
**deliver the core idea** — the trustless economic cage. Don't stop at "tests pass";
confirm the thesis holds.

---

You are auditing the Bound Protocol hackathon project. Work dir: `/Users/dogu/bound`.
Read `PROJECT.md` first (vision + "Trust Model & Known Limitations") and `.env.testnet`
(source of truth for addresses/secrets; never print secret keys). Then run the checks
below and produce a single PASS/FAIL report. Do NOT redeploy contracts or run the demo
unless a check fails and the user approves — those spend testnet funds and change addresses.

## Core idea you are verifying it matches
Bonding/insurance model, NOT a spending cap. An auditor **stakes their own capital** to
vouch that an agent's worst-case loss is bounded and pre-funded by a **locked reserve**.
A counterparty reads the certificate before transacting. If the vouch is false
(reserve short), ANYONE can challenge; for `InsufficientReserve` the ChallengeManager
**proves the fraud itself on-chain** (claimed reserve > actual balance) and in ONE
transaction slashes the auditor's stake, compensates the victim, and invalidates the cert.
The headline is the **trustless slash**, not a blocked payment.

---

## Part A — Does it WORK (mechanical)

1. `cargo test` → expect **21 tests pass** across 5 contracts (reserve-vault 3,
   auditor-staking 6, fee-escrow 2, challenge-manager 2, registry 8).
2. `cargo build --release --target wasm32-unknown-unknown` → 5 `.wasm` artifacts, no errors.
3. `pnpm verify-all` → runs typecheck + 4 live smoke suites. Expect all ✓:
   - tools-smoke / sdk-smoke: read a real cert + balances from testnet
   - mcp-smoke: 5 MCP tools exposed
   - routes-smoke: `/api/paid-service` (402→pay→200), `/api/verify` (400 + live cert),
     `/api/auditor` GET (pending params + current cert)
   - Note: a `⚠ ANTHROPIC_API_KEY EMPTY` line is EXPECTED unless a key was added —
     everything except live `/api/chat` streaming is key-independent. Not a failure.
4. `pnpm typecheck` alone → clean (whole repo, Next bundler config, target ES2020).
   - The live agent cert may read `status=Invalid` — that is EXPECTED (the demo slashed it).

## Part B — Does it deliver THE IDEA (thesis)

5. **Trustless slash is real, not cosmetic.** Open `contracts/challenge-manager/src/lib.rs`.
   Confirm `verify_insufficient_reserve` reads `claimed` from `Registry.get_cert_reserve`
   and `actual` from `ReserveVault.get_balance` via cross-contract calls and returns
   `actual < claimed` — i.e. the contract proves fraud itself (no oracle, no params from
   the challenger). Confirm `settle_fraud` slashes the auditor's **live** stake (80/20
   victim/challenger), drains the reserve to the victim, and invalidates the cert in one tx.

6. **The auditor's stake is actually bonded (the fix).** This is the property that makes
   "skin in the game" real. Confirm BOTH layers:
   - Code: `Registry.attest` calls `auditor_staking.lock(auditor, cert.expires_at)`;
     `AuditorStaking.release` reverts with `stake_locked` while `now < locked_until`.
   - Unit: `cargo test -p auditor-staking` includes `test_release_blocked_while_bonded_to_cert`
     (panics `stake_locked`) and `test_release_allowed_after_lock_expires`.
   - Live (read-only, no mutation): read `locked_until(auditor)` from the deployed
     AuditorStaking contract and confirm it is a future timestamp (≈ cert expiry). Then
     simulate `release --auditor <AUDITOR_ADDRESS>` and confirm it **traps/reverts**
     (Soroban surfaces the panic as `WasmVm, InvalidAction` in `release`). Use the
     addresses/secrets from `.env.testnet`. Do NOT submit any state-changing tx.

7. **Reserve is genuinely locked.** In `contracts/reserve-vault/src/lib.rs` confirm
   `release_to_operator` reverts (`reserve_still_locked`) before `unlock_at`, and
   `release_to_victim` / `slash` are auth-gated to the ChallengeManager only.

8. **Honesty of the claim.** Re-read PROJECT.md "Trust Model & Known Limitations" and
   confirm each stated limitation still matches the code:
   - victim is named by the challenger, not verified on-chain;
   - one reserve vault = one operator (no per-cert segregation);
   - `bound` is attested, never read by any contract (`reserve < bound` ⇒ only the
     reserve is pre-funded; the rest rests on the locked auditor stake);
   - forfeited challenge bond stays in ChallengeManager.
   Flag any NEW gap not already documented. If a limitation is no longer true, say so.

## Part C — Safety / leaks

9. `git check-ignore .env.testnet` → must print the path (ignored). Confirm `.gitignore`
   excludes `.env*`, `*.bak*`, `node_modules/`, `target/`, `.next/`. No secret key value
   should ever appear in your report or in any tracked file.

## Report format
A table of PASS/FAIL per numbered check with the one-line evidence (count, status, or the
revert reason). Then a 2-line verdict: (1) does it WORK, (2) does it MATCH THE IDEA —
including whether any undocumented gap was found. If anything fails, propose the fix but
do not redeploy or spend testnet funds without explicit approval.
