# @bound/mcp

An [MCP](https://modelcontextprotocol.io) server that gives any MCP-capable agent —
Claude Desktop, Claude Code, Cursor, your own client — the tools of
[Bound Protocol](https://github.com/iamnotdou/bound): verify an agent's certificate
before transacting with it, pay for a service autonomously over x402, read the meter
that records what a certificate has spent, price and buy coverage, and prove a false
attestation on-chain so the auditor is slashed.

It is the same tool table the Bound dashboard drives through the AI SDK, defined once
and adapted twice. Everything it does goes through [`@bound/sdk`](../sdk), which is a
regular dependency — there is nothing else to install.

## Usage

```bash
npx @bound/mcp
```

The server speaks MCP over stdio, so it is normally launched by a client rather than by
hand. `stdout` is the JSON-RPC channel; every diagnostic goes to `stderr`.

### Claude Desktop

`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS,
`%APPDATA%\Claude\claude_desktop_config.json` on Windows:

```json
{
  "mcpServers": {
    "bound": {
      "command": "npx",
      "args": ["-y", "@bound/mcp"],
      "env": {
        "AGENT_SECRET": "S...",
        "OPERATOR_SECRET": "S...",
        "AUDITOR_SECRET": "S...",
        "CHALLENGER_SECRET": "S...",
        "COUNTERPARTY_SECRET": "S..."
      }
    }
  }
}
```

### Cursor

`.cursor/mcp.json` in the project, or `~/.cursor/mcp.json` globally — same shape:

```json
{
  "mcpServers": {
    "bound": {
      "command": "npx",
      "args": ["-y", "@bound/mcp"],
      "env": {
        "AGENT_SECRET": "S...",
        "OPERATOR_SECRET": "S...",
        "AUDITOR_SECRET": "S...",
        "CHALLENGER_SECRET": "S...",
        "COUNTERPARTY_SECRET": "S..."
      }
    }
  }
}
```

Claude Code takes the same block in `.mcp.json`, or `claude mcp add bound -- npx -y
@bound/mcp`.

## Environment

| Variable              | Needed for                                                              |
| --------------------- | ----------------------------------------------------------------------- |
| `AGENT_SECRET`        | `execute_payment`, `fetch_paid_service`, `fund_float`, `enroll_agent`   |
| `OPERATOR_SECRET`     | `enroll_agent`, `halt_certificate`, `resume_certificate`, `pay_premium` |
| `AUDITOR_SECRET`      | `claim_premium`                                                         |
| `CHALLENGER_SECRET`   | `challenge_certificate`                                                 |
| `COUNTERPARTY_SECRET` | reserved for the demo counterparty role                                 |
| `STELLAR_NETWORK`     | optional; only `testnet` exists today, and it is the default            |

Every key is read lazily, so a session that only reads never touches one and starts
fine with none of them set. Contract addresses and network endpoints are committed
inside `@bound/sdk` — there is nothing to configure.

**These are Stellar secret keys (`S...`).** They sign real transactions on testnet, and
the write tools spend from them without asking. Bound is testnet-only today (see
[Status](#status)), so there is no mainnet balance to lose — but treat the config block
as a credential file regardless. If you are running inside a checkout of the Bound repo,
the server will also pick the same five variables up from a `.env.testnet` found by
walking upward from its working directory.

## Tools

Read-only tools simulate against the chain, sign nothing and spend nothing; they are
marked `readOnlyHint` so a client can run them without prompting. Everything else signs
with one of the keys above.

| Tool                       | Read | What it does                                                                                                                                         |
| -------------------------- | :--: | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `verify_agent_certificate` |  ✓   | The headline check: is this agent's certificate valid, and what bound, reserve and auditor stake back it                                             |
| `get_balance`              |  ✓   | USDC balance of an address; defaults to the agent                                                                                                    |
| `get_routing_status`       |  ✓   | Whether an address's payments are metered, which certificate against, and the float it holds                                                         |
| `get_cert_meter`           |  ✓   | A certificate's routed spend, float, float cap and halt state, next to the bound they measure against                                                |
| `quote_premium`            |  ✓   | Price coverage — for a published certificate, or a hypothetical bound × duration                                                                     |
| `get_coverage`             |  ✓   | Premium paid, protocol fee, and how much of the auditor's yield has accrued and is claimable                                                         |
| `execute_payment`          |      | Send USDC. Routed through the meter when the agent is enrolled, raw USDC when it is not                                                              |
| `fetch_paid_service`       |      | x402: fetch a URL, and if it answers `402`, pay the demanded price and retry — no human approves it                                                  |
| `enroll_agent`             |      | Bind the agent to a certificate and set its float cap. Operator **and** agent both authorize                                                         |
| `fund_float`               |      | Move USDC into the router's custody to pay from                                                                                                      |
| `halt_certificate`         |      | Operator's kill switch: stop every transfer, withdrawal and burn on a certificate                                                                    |
| `resume_certificate`       |      | Lift a halt                                                                                                                                          |
| `pay_premium`              |      | Operator buys coverage. Once per certificate, and only after an auditor has attested                                                                 |
| `claim_premium`            |      | Auditor withdraws accrued-and-unclaimed yield                                                                                                        |
| `challenge_certificate`    |      | Prove an attestation false. For `InsufficientReserve` the contract verifies the fraud itself, slashes the auditor's stake and compensates the victim |

Two things are worth knowing before an agent acts on these:

- **There is no spending cap.** The agent pays what it is asked. What protects the
  counterparty is the certificate — a pre-funded reserve and an auditor's slashable
  stake bounding the worst case — not a limit on the call.
- **Routed spend is gross flow, not loss.** A certificate that has routed more than its
  bound has broken a covenant about its own conduct; it has not thereby cost anyone that
  much money. `get_cert_meter` returns the bound alongside the counter so the comparison
  is explicit rather than implied.

## Status

Stellar **testnet only**. The contract addresses, network endpoints and demo actor
public keys are committed inside `@bound/sdk`; there is no mainnet deployment yet.

## Embedding it

The package root exports the pieces, for callers who want them somewhere other than a
stdio process:

```ts
import { createBoundMcpServer } from "@bound/mcp";

const server = createBoundMcpServer();
await server.connect(myOwnTransport);
```

`@bound/mcp/tools` is a second, lighter entry point carrying only the tool table —
`{ description, parameters (a Zod raw shape), execute }` per tool — with no MCP server
attached. Import that to drive the same tools from a framework that is not MCP; the
Bound dashboard uses it to build AI SDK tools.

Both entry points are **server-side only**: the write tools sign with secret keys from
the environment, so they belong behind your own backend, never in a browser bundle.

## Learn more

Source, contracts, and full documentation: <https://github.com/iamnotdou/bound>
