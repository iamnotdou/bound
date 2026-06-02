# Bound Protocol — Frontend Technical Plan

> Greenfield UI on a finished backend. Goal: a hackathon-ready web app where the
> **full demo runs live on Stellar testnet** and the climax (trustless slash) is
> visible on screen. Installs are CLI-driven (shadcn-style). No new protocol work —
> only a thin read-shim the UI needs (§3).

---

## 0. Readiness gate — ✅ RUN & GREEN (2026-06-03)

The frontend is worthless if the chain calls fail mid-demo. These were run and all pass:

```bash
cargo test                       # ✅ 21 passed (reserve 3 · auditor 6 · fee 2 · challenge 2 · registry 8)
pnpm verify-all                  # ✅ typecheck clean + tools/sdk/mcp/routes smoke all green (live testnet)
```

`verify-all` exercises every API route the UI consumes — now including the new
`/api/ledger` (live: 5 balances + reserve held/claimed + stake + cert) and `/api/cheat`
(simulate-only). A `⚠ ANTHROPIC_API_KEY EMPTY` line is expected and only affects live
`/api/chat` streaming — set the key before the demo.

**Live state confirmed (2026-06-03):** agent cert reads `status=Invalid`, reserve held `$0`,
auditor stake `$0`, counterparty `$122,800` — i.e. a prior demo already ran its slash. This
is the expected resting state. The UI's `/auditor` "Sign & Publish" re-seeds a fresh
`Verified` cert (re-staking the auditor) before each run — see §7 reset + §11.8. **Set the
ANTHROPIC key and run the reset before demoing.**

### ✅ Live full-loop dry-run PASSED (2026-06-03) — `scripts/dry-run.ts`
Ran the whole demo on testnet end to end. The thesis holds, on-chain, with real money:
- seed cert #4 → **Verified** (claims $10k) · operator funds **$4k** (the trap) · agent pays
  **$500** (tx `20032b67…a4c2`)
- challenger `resolve()` → **FRAUD_PROVEN**: victim **+$5,200** ($4k reserve + 80% of the
  $1.5k stake), challenger **+$300** (20% finder's fee), auditor stake **→ $0**, cert
  **Verified → Invalid**, one transaction, no oracle.

Re-run anytime with: `pnpm dlx ts-node --transpile-only --project scripts/tsconfig.json scripts/dry-run.ts`

### ✅ DEPLOYMENT DRIFT — RESOLVED by redeploy (2026-06-03)
A drift was found and fixed: the previously-deployed contracts predated the stake-lock +
reserve-lock fix (the `auditor_staking` binding lacked `lock`/`locked_until`). **All 5
contracts were redeployed from current source, bindings regenerated, `.env.testnet` + README
updated.** The lock guarantees are now LIVE on testnet — re-verified end to end:

- `withdraw-stake` → **reverts `stake_locked`** (auditor can't walk staked capital; surfaces
  as `HostError WasmVm InvalidAction`, exactly as VERIFY.md predicts).
- `withdraw-reserve` → **reverts `reserve_still_locked`** (operator can't reclaim early;
  fresh vault `unlock_at` = now + 365d).
- slash still works: victim **+$5,200**, challenger **+$300**, stake → $0, cert → Invalid.

New addresses (also in README + `.env.testnet`): Registry `CBM2UAVZ…AJWV` · ReserveVault
`CDN6S5DK…4WFB` · AuditorStaking `CCSJTEXO…4VZB` · FeeEscrow `CD4EZ5FC…EYOF` ·
ChallengeManager `CANDUKOY…UNZH`.

> Two tooling gotchas captured for next time: (1) `stellar contract bindings typescript`
> writes a `package.json` with `"type":"module"` that breaks the ts-node (CJS) smoke scripts
> — `git checkout` the binding `package.json` files after regen (the contract id lives in
> `src/index.ts`). (2) The simulate-cheat helper must read the AssembledTransaction `.result`
> to surface a trapped simulation — with a signer configured, `build()` alone won't reject.
> Both fixed in `bound-client.ts`.

**Backend confirmed during planning:**
- AI SDK `ai@4.3.19`, `@ai-sdk/anthropic@1.2.12`. `useChat` is exported from `ai/react`.
- Tool calls stream as `message.parts[]` with `type: 'tool-invocation'`, each carrying
  `{ toolName, state: 'partial-call'|'call'|'result', args, result }`. ToolCallCard reads this.
- `Registry.get_cert_id(agent)` exists on-chain → certId is recoverable (§3).
- Public addresses (operator/agent/auditor/counterparty/usdc) are already injected to the
  browser via `next.config.ts env`. **CHALLENGER stays server-side — keep it that way.**

---

## 1. Stack & decisions (locked)

| Concern | Decision |
|---|---|
| Framework | Next 15.3 App Router, React 19 (already installed) |
| Styling | Tailwind v4 (installed; **postcss config missing — add it**) |
| Component kit | **shadcn/ui** via CLI (`npx shadcn@latest`), latest CLI supports TW v4 + React 19 |
| Chat | AI SDK v4 `useChat` from `ai/react` against existing `/api/chat` |
| Pkg mgr | pnpm 10 |
| Aliases | `@/*` → repo root (existing). shadcn aliased **under `app/`** (§2) |
| Wallets | none — demo keypairs are server-side by design |
| New backend | certId read-shim (§3) + 3–4 thin proof routes for `/control` (§11); chat/verify/auditor/paid-service reused as-is |

**Why not Vercel `ai-elements`:** it assumes AI SDK **v5** message shapes. We're on v4.
Build the chat from shadcn primitives + a custom `ToolCallCard` to avoid a version-mismatch
rabbit hole during crunch. (Revisit only if we upgrade `ai` to v5, which we won't pre-demo.)

---

## 2. Install sequence (CLI scripts)

Run in order. Each is copy-paste.

```bash
# 2.1 Tailwind v4 postcss (currently absent — Tailwind won't compile without it)
#   create postcss.config.mjs:  export default { plugins: { "@tailwindcss/postcss": {} } }

# 2.2 globals.css — the ONLY place tokens live (swap when visual sources land)
#   app/globals.css:  @import "tailwindcss";  + @theme { ...tokens... }

# 2.3 shadcn init (writes components.json, lib/utils cn(), base CSS vars)
pnpm dlx shadcn@latest init
#   When prompted / via components.json, set aliases UNDER app/ to match our tree:
#     components → @/app/components
#     ui         → @/app/components/ui
#     lib        → @/app/lib
#     utils      → @/app/lib/utils          (client-safe cn(); separate from config.ts)
#     hooks      → @/app/hooks
#   base color: neutral/slate (placeholder until visual direction is provided)

# 2.4 Add only the primitives we use
pnpm dlx shadcn@latest add button card input badge separator scroll-area \
  skeleton sonner tooltip tabs dialog avatar

# 2.5 If pnpm + React 19 peer warnings block a component, re-run that add with:
#   pnpm dlx shadcn@latest add <name>   (shadcn writes source, no runtime peer pins)
```

**components.json caveat:** our `tsconfig` maps `@/*` to the **repo root**, so
`@/app/components/ui/button` resolves to `./app/components/ui/button.tsx`. Keep that
mapping consistent in `components.json` or shadcn will scaffold into a root-level
`components/` you don't want. `app/lib/utils.ts` (the `cn` helper) is client-safe and
must NOT import `app/lib/config.ts` (that one pulls secrets/dotenv).

---

## 3. Backend shim + proof routes — ✅ DONE (verified live)

All backend the UI needs is implemented and confirmed against testnet via
`pnpm routes-smoke`. Nothing here is left for the UI build.

**certId shim (the chat climax needs it; `verify()` doesn't return it):**
- `app/lib/bound-client.ts` → `certIdForAgent(agent)` (wraps on-chain `get_cert_id`, returns null if none).
- `app/lib/cert-view.ts` → `CertView.certId` + optional `certId` param on `toCertView`.
- `app/api/verify/route.ts` + `app/api/auditor/route.ts` → now return `certId`. Any page that
  verifies self-discovers the certId; the chat needs no operator input to challenge.

**Proof routes for `/control` (§11):**
- `GET  /api/ledger`     → 5 balances + reserve held/claimed + auditor stake + cert view. Pure reads, pollable. ✅ live
- `POST /api/operator`   → `deposit-reserve` (amount-controlled, default $4k trap) · `deposit-fee` · `publish`. ✅ wired
- `POST /api/challenger` → `challenge` (challenge+resolve), returns victim before/after balance + cert flip. ✅ wired
- `POST /api/cheat`      → simulate-only `withdraw-reserve` / `withdraw-stake`; reverted=true IS the proof. ✅ live
- `bound-client` also gained `simulateReleaseReserve` / `simulateReleaseStake` (build+simulate, never sign).

**State-dependency found while verifying (matters for the demo):** the `reserve_still_locked`
and `stake_locked` guards only bind while a cert is **Verified + unexpired**. With the live
cert currently `Invalid` (post-slash) and reserve/stake at `$0`, the cheat simulations
legitimately *succeed* (nothing is locked). So the adversarial cheat lane only proves the
cage after a fresh seed — see §11.5 + the pre-demo checklist. `routes-smoke` asserts the
mechanism always, and asserts `reverted===true` only when the cert reads Verified.

---

## 4. File tree (target)

```
postcss.config.mjs                     # 2.1
components.json                        # shadcn (2.3)
app/
  globals.css                          # tokens (2.2)
  layout.tsx                           # shell: <Nav/>, font, <Toaster/>, metadata
  page.tsx                             # Landing  (/)
  dashboard/page.tsx                   # Cert lookup (/dashboard)        [client]
  chat/page.tsx                        # MAIN DEMO (/chat)               [client]
  auditor/page.tsx                     # Sign & seed / reset (/auditor)  [client]
  control/page.tsx                     # PROOF cockpit — 5 actor lanes (/control) [client]  §11
  api/
    ledger/route.ts                    # GET — all balances + cert + stake + reserve  §11
    operator/route.ts                  # POST deposit-reserve|deposit-fee|publish      §11
    challenger/route.ts                # POST challenge (challenge+resolve)            §11
    cheat/route.ts                     # POST simulate-only reverts (optional)         §11
  hooks/
    useCert.ts                         # fetch+poll /api/verify (status flip)
    useLedger.ts                       # fetch+poll /api/ledger (the proof board)      §11
    useAuditorPending.ts               # GET /api/auditor
  lib/
    utils.ts                           # cn() (shadcn)
    ui-config.ts                       # public addrs + contracts + explorer links (client-safe)
    explorer.ts                        # stellar.expert URL builders (tx / account / contract)
  components/
    ui/...                             # shadcn primitives (2.4)
    Nav.tsx
    AddressPill.tsx                    # truncate + copy + explorer link
    UsdAmount.tsx                      # renders the "$10,000" strings the API returns
    StatusBadge.tsx                    # Verified / Pending / Invalid / Slashed
    ContractRow.tsx                    # the 5 contracts + USDC, linked
    CertificateCard.tsx               # SHARED: dashboard + chat side-panel + auditor
    chat/
      ChatPanel.tsx                    # useChat wrapper, message list, composer
      MessageBubble.tsx               # text parts
      ToolCallCard.tsx                # per tool-invocation, state-colored  ← the wow
      QuickPrompts.tsx               # 4 preset buttons
      CertSidePanel.tsx              # live cert; flips Verified→Invalid on slash
    control/                          # §11 proof cockpit
      LedgerBoard.tsx                 # 5 accounts + contracts, Δ flashes per action
      ActorLane.tsx                   # one actor's buttons; posts a call, refetches ledger
      ActionResultCard.tsx            # tx hash / revert reason, green/red
      CheatLane.tsx                   # optional: "try to cheat" → expected revert
```

`ui-config.ts` reads only the browser-exposed env (`process.env.AGENT_ADDRESS`, etc. via
`next.config.ts`) — never imports `config.ts`/`accounts.ts`.

---

## 5. Component contracts (the parts that matter)

### `ToolCallCard.tsx` — the demo's visual payload
Input: one tool-invocation from `message.parts`. Branch on `toolName`:

| toolName | rendering | accent |
|---|---|---|
| `verify_agent_certificate` | mini cert: status, bound, reserve, auditor stake, auditor pill | neutral/green if valid |
| `get_balance` | address pill + USDC | neutral |
| `execute_payment` | amount → recipient pill, **tx hash chip → stellar.expert** | green |
| `fetch_paid_service` | "paid $X autonomously (no human approved)", tx chip, returned body | amber |
| `challenge_certificate` | **claimed $10k vs actual on-chain**, slash split 80/20, status → INVALID | red |

State handling: `partial-call`/`call` → animated "running…" skeleton; `result` → final
card from `toolInvocation.result`. Every tx hash is an explorer link. The
`challenge_certificate` result card is the single highest-effort component — it is the
moment judges remember.

### `CertSidePanel.tsx` — proves the flip is real
Uses `useCert(agentAddress)` to poll `/api/verify` (e.g., every 4s, and force-refetch when
a `challenge_certificate` tool result arrives). Shows status badge prominently so the
audience watches **Verified → Invalid** the instant the slash settles. This sync is the
difference between "a chatbot said it slashed" and "the chain shows it slashed."

### `CertificateCard.tsx` — one component, three homes
Props: a `CertView`. Renders status badge, bound, reserve, auditor stake, auditor pill,
expiry, and (if present) certId. Reused verbatim by dashboard, chat side-panel, auditor.

---

## 6. Pages

### `/` Landing — static, no fetch
Hero ("Know your worst case before you transact") → one-line value prop → 4-step "how it
works" (lock reserve → auditor vouches with own money → read before you transact → anyone
catches a lie & gets paid) → `<ContractRow/>` with live testnet links → CTA → `/chat`.

### `/dashboard` — client
Address `Input` + "Use demo agent" prefill (env `AGENT_ADDRESS`) → `POST /api/verify` →
`CertificateCard`. Handle 400 (bad address, inline) and 502 (chain read, toast). The
standalone "read before you transact" surface.

### `/chat` — the judged demo, client
`ChatPanel` (useChat → `/api/chat`) on the left, `CertSidePanel` on the right.
`QuickPrompts`: **Verify certificate · Pay $500 · Access x402 service · Challenge the bad
attestation**. On mount, the page fetches the current cert + certId (via verify shim §3)
so the Challenge prompt can be issued with the real certId and counterparty/victim address
without operator input. Tool calls render as `ToolCallCard`s; tx hashes link out.

### `/auditor` — client, doubles as the **demo reset**
`GET /api/auditor` → pending attestation panel (operator, agent, bound **$50k**, claimed
reserve **$10k**, stake **$1.5k**) + current `CertificateCard`. "Sign & Publish" →
`POST {action:"sign-publish"}` → fresh `Verified` cert (re-stakes auditor if a prior demo
zeroed them). Run this before every demo to reset status to Verified.

---

## 7. Full demo run-of-show (what the UI must make work, live)

> Pre-funded testnet accounts. Reserve vault holds **< the claimed $10k** (true by default,
> and still true after any prior slash) → the challenge always proves `actual < claimed`.

1. **Reset** — `/auditor` → *Sign & Publish* → cert = **Verified**, claims $10k reserve.
2. **Read** — `/dashboard` (or chat side-panel) → verify agent → Verified, $50k bound, $10k reserve, $1.5k stake.
3. **Pay** — `/chat` → *Pay $500* → green `execute_payment` card + tx chip (real testnet tx).
4. **Autonomous x402** — *Access premium service* → 402 → agent pays → amber card ("no human approved the amount").
5. **CLIMAX** — *Challenge the bad attestation* → red `challenge_certificate` card:
   claimed $10k vs actual on-chain, auditor $1.5k **SLASHED** (80% victim / 20% challenger),
   reserve drained to victim, cert → **INVALID**. Side-panel flips **Verified → Invalid**.
   Counterparty walks with ~$5,200. *"No oracle. No arbiter. Arithmetic."*

### Pre-demo checklist (run 5 min before)
- [ ] `pnpm verify-all` green (chain reachable, routes live).
- [ ] `ANTHROPIC_API_KEY` set in `.env.testnet` (required for live `/api/chat`).
- [ ] `/auditor` Sign & Publish done → cert reads **Verified** on `/dashboard`.
- [ ] Agent USDC balance ≥ ~$700 (covers the $500 pay + x402 price). Challenger funded for the $100 bond.
- [ ] Reserve vault balance **< $10k** (so the slash proves fraud). Default/post-slash state satisfies this.
- [ ] Open `/chat`, confirm the 4 quick prompts are present and certId loaded.
- [ ] **If demoing the cheat lane (§11.5): do it right after the `/auditor` seed, while the
      cert reads Verified** — that's the only state where the locks bind and the reverts show.
      Verified live: pre-seed (cert `Invalid`, reserve `$0`) → cheats correctly do NOT revert.

### Resilience (testnet is flaky under stage wifi)
- Every chain call wrapped: loading skeleton → success → typed error toast with retry.
- `useChat` `onError` surfaces a toast and keeps the composer usable.
- If RPC times out mid-demo, the actions are idempotent enough to re-run; the side-panel
  re-polls and self-heals to true on-chain state.
- Fallback narrative if live chain stalls: `pnpm demo` (the 8-step CLI) is the backup proof
  the slash works — keep a terminal open as insurance.

---

## 8. Build order & estimate

| # | Step | Output | ~time |
|---|---|---|---|
| 0 | Readiness gate (§0) | ✅ DONE — 21 tests, typecheck, `verify-all` all green | — |
| B | **All backend: certId shim + ledger/operator/challenger/cheat routes (§3, §11.6)** | ✅ DONE — verified live via `routes-smoke` | — |
| 1 | postcss + globals + shadcn init + add primitives (§2) | Tailwind compiles, kit ready | 0.5h |
| 3 | Shell: layout, Nav, ui-config, explorer, AddressPill, UsdAmount, StatusBadge | shared chrome | 1h |
| 4 | CertificateCard + `/dashboard` + useCert (reads `certId` from `/api/verify`) | first real chain read on screen | 1h |
| 5 | `/auditor` (seed + reset) | demo can be reset repeatably | 1h |
| 6 | **`/chat`: ChatPanel + ToolCallCard + QuickPrompts + CertSidePanel** | the demo | 3–4h |
| 7 | `useLedger` + `LedgerBoard` (route done) | the proof board | 0.5h |
| 8 | `/control` actor lanes wiring the done routes (§11) | drive all 5 actors | 1.5h |
| 9 | *(optional)* `CheatLane` UI wiring `/api/cheat` (route done) | adversarial proof | 0.5h |
| 10 | `/` landing + ContractRow | submission-clickable | 1.5h |
| 11 | Polish pass once visual sources land: swap `@theme` tokens, slash-moment motion | final look | 1.5h |

Backend (rows 0/B) is finished and verified — **everything left is UI wiring** over routes
that already return live data. Critical path is step 6. Steps 7–9 (§11) are the multi-actor
proof harness; the routes behind them are done, so they're now pure front-end.

## 9. Definition of done (submission gate)
- [ ] `cargo test` (21) + `pnpm verify-all` green.
- [ ] All 5 pages build and deploy (Vercel) with public addresses linked to stellar.expert.
- [ ] `/chat`: all 4 quick prompts execute live; tool cards render with real tx hashes.
- [ ] Climax: challenge slashes the auditor and the side-panel flips Verified → Invalid on screen.
- [ ] `/control`: all 5 actor lanes drive live calls; the Ledger board shows the value move,
      and the two-outcome toggle (under-funded → slash, funded → bond forfeit) both work.
- [ ] `/auditor` reliably resets the cert to Verified between runs.
- [ ] No secret key reaches the browser (challenger + all secrets server-side only).
- [ ] README updated with the live Vercel URL.

## 10. Open items (waiting on you)
- **Visual direction** — MCPs/sources you'll provide. Slots into `app/globals.css @theme` +
  shadcn base color only; no component rewrites.
- Confirm we add the certId to the **verify route** (recommended) vs. a separate `/api/cert-id`.
- Confirm chat auto-seeds on mount (recommended, robust to a cold page open) vs. requiring
  `/auditor` to be run first.
- **`/control`**: ship as its own page (recommended — tester audience ≠ judge audience)?
  And include the optional adversarial **cheat lane** (step 9)?

---

## 11. Proof cockpit — exercising all 5 actors on one screen

> A bonding protocol isn't proven by the happy path. It's proven by (a) value actually
> moving between 5 *named* accounts on live testnet, and (b) every actor's defection being
> punished on-chain — the Nash table from `PROJECT.md`, made clickable. `/control` is that
> harness: `/chat` is the narrated demo; `/control` is the deterministic proof.

### 11.1 The Ledger board — the proof surface
One `GET /api/ledger` (pure reads, safe to poll ~4s) returns everything the board renders:

```
Operator     $… USDC          Reserve vault:   $4,000  (claims $10,000)
Agent        $… USDC          Auditor stake:   $1,500  locked → cert expiry
Auditor      $… USDC          Cert #42 status: ● VERIFIED
Counterparty $… USDC          Fee escrow:      $500
Challenger   $… USDC          ← Δ column flashes green/red after each action
```
Wraps `usdcBalance`×5 (operator/agent/auditor/counterparty/challenger), `reserveBalance`,
`auditorStake`, `verifyCertificate` (+ `certIdForAgent` §3). Every actor button refetches it,
so each click produces a *visible state diff* — the diff is the proof.

### 11.2 Actor lanes — control → call → what it proves

| Actor | Button | Call | Ledger proof / incentive |
|---|---|---|---|
| **Operator** | Deposit reserve *(amount slider, set $4k)* | `depositReserve` | Vault ↑. **Sets the trap**: claim $10k, fund $4k |
| | Deposit fee $500 | `depositFee` | Fee escrow ↑ |
| | Publish cert (bound $50k, claim $10k) | `publishCertificate` | Cert **● PENDING** |
| **Auditor** | Stake $1,500 | `stakeAsAuditor` | Auditor ↓, stake ↑, registered |
| | Attest | `attestCertificate` | Cert **PENDING → VERIFIED**; stake `locked_until` = expiry |
| **Counterparty** | Verify → Accept | `verifyCertificate` (read) | Decision panel: "worst-case $50k, pre-funded $10k *claimed*, auditor risks $1.5k" |
| **Agent** | Pay $500 → counterparty | `executePayment` | Agent ↓, Counterparty ↑, **real tx hash** |
| | Pay x402 service | `fetch_paid_service` | "no human approved the amount", tx hash |
| **Challenger** | Challenge (InsufficientReserve) | `challengeCertificate` (challenge+resolve) | **CLIMAX** ↓ |

### 11.3 The climax on the board (one tx, settled by arithmetic)
```
Auditor stake   $1,500 → $0        SLASHED
Counterparty    +$1,200 (80%) + $4,000 reserve  → +$5,200
Challenger      +$300   (20% finder's fee) + bond returned
Cert #42        VERIFIED → ● INVALID
```
No oracle, no arbiter — the contract read claimed $10k vs actual $4k itself.

### 11.4 The two-outcome toggle — why it's proof, not a script
Same Challenger button, opposite result, decided purely on-chain:
- Reserve **under-funded** ($4k) → `resolve()`: `actual < claimed` → **slash**.
- Reserve **fully funded** ($10k) → `resolve()`: `actual ≥ claimed` → **challenger forfeits the $100 bond**, cert stays VERIFIED.

Run both in front of the judge. The outcome flips because the *chain* decided. Most
convincing 30 seconds in the demo.

### 11.5 Adversarial "cheat" lane (optional, step 9) — the real protocol proof
Each Nash-table defection as a button that **reverts on-chain** (simulate-only — reads the
revert, spends nothing):

| Cheat | Expected result |
|---|---|
| Operator: withdraw reserve before expiry | reverts `reserve_still_locked` |
| Auditor: release stake after attesting | reverts `stake_locked` |
| Challenger: challenge a *funded* cert | forfeits bond (no fraud found) |

Backend is **done** (`simulateReleaseReserve`/`simulateReleaseStake` + `POST /api/cheat`,
verified live on the redeployed contracts — both defections revert while the cert is
Verified). A red "REVERTED: stake_locked" card proves the cage holds without any
happy-path narrative.

> ⚠ **State-dependent — verified on testnet.** These locks only bind while the cert is
> **Verified + unexpired**. When the cert is `Invalid` (post-slash) with reserve/stake at
> `$0`, the simulations succeed and `reverted=false` — *correctly*, because nothing is
> locked. So **run the cheat lane only after a fresh `/auditor` seed**, while the side-panel
> reads Verified. Out of order it under-sells the protocol (looks like the lock failed).
> The right demo order: seed → show cheats revert → pay → challenge (which then makes the
> cheats succeed again, because the cert is now Invalid — itself a teachable beat).

### 11.6 New routes (all thin, additive)

| Route | Method | Wraps | Notes |
|---|---|---|---|
| `/api/ledger` | GET | balances ×5, reserve, stake, verify, certId | the proof board; pure reads, pollable |
| `/api/operator` | POST `{action: deposit-reserve\|deposit-fee\|publish, amountUsd?}` | `depositReserve`/`depositFee`/`publishCertificate` | drives Operator lane; signs operator key server-side |
| `/api/challenger` | POST `{certId?, victim?}` | `challengeCertificate` | certId auto-discovered if omitted; victim defaults to env counterparty |
| `/api/cheat` | POST `{action}` | `simulateReleaseReserve`/`simulateReleaseStake`, **simulate-only** | `reverted=true` only while cert is Verified (§11.5); no state change |

All signing stays server-side; the browser only POSTs an intent. Auditor/Agent/Counterparty
lanes reuse existing routes (`/api/auditor`, `/api/chat` tools, `/api/verify`).

### 11.7 Full sequenced run (touches all 5, ~90s)
```
1 Operator    deposit reserve $4k · fee $500 · publish     → cert PENDING
2 Auditor     stake $1,500 · attest                        → cert VERIFIED, stake locked
3 Counterparty verify → Accept                             → reads the live cert
4 Agent       pay $500 → counterparty · pay x402           → tx hashes, balances move
5 Challenger  challenge InsufficientReserve                → SLASH: stake→0, victim +$5,200, INVALID
  (Reset → re-run step 5 against a FUNDED reserve          → challenger forfeits bond)
```
Every step changes the Ledger. By the end the audience has *watched* $5,200 move from a
lying auditor's stake + a short reserve into the victim — the entire thesis.

### 11.8 Reset
"Reset demo" = `/auditor` Sign & Publish (re-stakes the auditor — the slash zeroed it — and
re-publishes a fresh VERIFIED cert claiming $10k). This is what makes the loop repeatable.
```
