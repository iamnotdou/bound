# @bound/near

Check an AI agent's Bound certificate on NEAR before you deal with it.

Bound is a surety layer for autonomous agents. An operator locks a reserve,
an auditor stakes behind the agent, and the agent's account holds only one
kind of key: a function-call key that can call `pay` on the Bound contract
and nothing else. Payments go through a metered float. If the agent harms
someone, an arbiter can pay victims out of the reserve and the auditor's
stake.

This package gives you three ways to read that state:

- a **CLI**, `bound-near`, that answers "deal or refuse" for an agent account,
- a **TypeScript library** for services that gate requests on a certificate,
- an **MCP server** so an LLM agent can run the same checks itself.

Reads need no NEAR account and no key. **Testnet only.** The token is a mock
USDC with 6 decimals and open mint, so it is play money.

- Live demo: https://near.boundprotocol.dev
- Docs: https://near.boundprotocol.dev/docs
- Source: https://github.com/iamnotdou/bound/tree/main/packages/near

## Install

```sh
npm install @bound/near
```

You need Node.js 20 or later. The testnet deployment (contract
`bound-v3.boundproto-nh26.testnet`, token `usdc.boundproto-nh26.testnet`, RPC
endpoints and two demo agents) ships inside the package, so there is nothing
to configure.

## CLI

```sh
npx @bound/near verify bound-agent2-mwfnc6q8.testnet
```

```text
bound-agent2-mwfnc6q8.testnet: cert 1 Verified | ... | coverage 130 | spent 1 float 9/10 | ...
  coverage 130 USDC = reserve 30 + auditor allocation 100 (...)
  confinement: confined (1 key(s), no contract code, no global contract)
  decision: deal (Verified)
```

| Command                              | What it does                                                       | Exit code                     |
| ------------------------------------ | ------------------------------------------------------------------ | ----------------------------- |
| `bound-near verify <agent> [--json]` | Shows the contract's verdict, coverage and the agent's confinement | 0 deal, 2 refuse, 1 error     |
| `bound-near cert <id> [--json]`      | Reads one certificate by id                                        | 0 found, 2 not found, 1 error |
| `bound-near mcp`                     | Starts the MCP server on stdio. stdout carries JSON-RPC only       | runs until stdin closes       |

Options:

- `--contract <id>` sets the Bound contract. The default is the bundled testnet deployment.
- `--rpc <url>` sets the RPC endpoint. You can repeat it, and the URLs are tried in order. The default is FastNEAR testnet.

Try the settled demo agent to see a refusal. Its certificate is `Invalid`
after a fraud settlement paid its victim:

```sh
npx @bound/near verify bound-agent-mwfnc6q8.testnet   # decision: refuse, exit 2
```

### What "deal" means

`deal` is true only when the certificate is `Verified`, is not halted, and
still has float to pay from. Confinement is a separate check, because the
contract cannot see the agent's keys or code. An agent is confined when both
of these hold:

- every access key is a function-call key for the Bound contract, limited to `pay`;
- the account runs no code. That means `code_hash` is all ones and no NEAR
  global contract is attached (`global_contract_hash` and
  `global_contract_account_id` are both empty).

Code on the account, whether local or global, could move funds without any
key. Read both the verdict and confinement before you extend credit.

## Library

```ts
import { TESTNET, fmtUsdc, getCertificate, verifyAgent } from "@bound/near";

const r = await verifyAgent("bound-agent2-mwfnc6q8.testnet");

if (r.deal && r.confinement?.confined) {
  console.log(`deal: coverage ${fmtUsdc(r.coverage)} USDC on ${r.contract}`);
} else {
  console.log(`refuse: ${r.reason}`);
}

const cert = await getCertificate(TESTNET.demo.settledCertId);
console.log(cert?.status, cert?.settlement);
```

Main exports:

- `verifyAgent(agent, opts?)` returns `{ deal, reason, certificate, coverage, confinement }`.
- `getAgentCertificate(agent, opts?)` and `getCertificate(id, opts?)` read the raw contract views.
- `checkConfinement(agent, opts?)` reads keys and code (local and global) and returns `confined` with the details.
- `assess`, `isConfined`, `keysConfined`, `hasNoCode`, `coverageOf`, `fmtUsdc`, `usdc` and `payEvents` are pure helpers with no network.
- `TESTNET` is the bundled deployment. It is also available as `@bound/near/testnet.json`.

`opts` takes `{ contractId?, rpc?, provider? }`, where `provider` is a
near-api-js `Provider`. Writes (`call`, `tryCall`) sign only with an
`Account` you build yourself. The package never reads key files.

## MCP server

Add this to your MCP client config. It works with Claude Desktop, Cursor and
any other client that launches stdio servers:

```json
{
  "mcpServers": {
    "bound-near": {
      "command": "npx",
      "args": ["-y", "@bound/near", "mcp"]
    }
  }
}
```

| Tool                    | Purpose                                                                                        |
| ----------------------- | ---------------------------------------------------------------------------------------------- |
| `near_verify_agent`     | The agent's certificate and a `deal` verdict                                                   |
| `near_get_certificate`  | One certificate by id                                                                          |
| `near_check_agent_keys` | Access keys plus local and global code, and `confined`                                         |
| `near_pay`              | Pays from the agent's float through `pay`. It is disabled unless you set an agent key (below). |

The three read tools need nothing. To let an agent pay, set
`BOUND_NEAR_AGENT_ID` and `BOUND_NEAR_AGENT_KEY` in the server's `env`. The
key must be the agent's function-call key, which NEAR lets call only `pay` on
the Bound contract. So even a prompt-injected model holding it can at worst
spend the float, and only to recipients the operator allowlisted.

You can also embed the server in your own process:

```ts
import { createNearMcpServer } from "@bound/near/mcp";
const server = createNearMcpServer({ contractId: "bound-v3.boundproto-nh26.testnet" });
```

## License

MIT
