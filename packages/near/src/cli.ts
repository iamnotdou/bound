#!/usr/bin/env node
// bound-near: read Bound certificates on NEAR testnet, or serve them to an
// LLM over MCP (stdio). Needs no key; `near_pay` in the MCP server is enabled
// only when BOUND_NEAR_AGENT_ID and BOUND_NEAR_AGENT_KEY are set.
//
//   bound-near verify <agent>   exit 0 deal, 2 refuse, 1 error
//   bound-near cert <id>        exit 0 found, 2 no such certificate
//   bound-near mcp              stdio MCP server; stdout is JSON-RPC only

import { parseArgs } from "node:util";
import { coverageOf, describeCert, describeCode, fmtUsdc, type CertViewJson } from "./format.js";
import { quietRpcRetryLogs } from "./quiet.js";
import { getCertificate, verifyAgent, type ReadOptions } from "./read.js";
import { TESTNET } from "./testnet.js";
import { VERSION } from "./version.js";

const HELP = `bound-near ${VERSION}: Bound certificates for AI agents on NEAR testnet

Usage
  bound-near verify <agent.testnet> [--json]   deal or refuse, coverage, confinement
  bound-near cert <id> [--json]                read one certificate
  bound-near mcp                               MCP server over stdio

Options
  --contract <id>   Bound contract (default ${TESTNET.contracts.bound.accountId})
  --rpc <url>       RPC endpoint, repeatable (default ${TESTNET.rpc.join(", ")})
  --json            machine-readable output
  -h, --help        this help
  -v, --version     print the version

Exit codes: 0 deal / found, 2 refuse / not found, 1 error.
Docs: https://near.boundprotocol.dev/docs`;

const json = (o: unknown) =>
  JSON.stringify(o, (_k, v) => (typeof v === "bigint" ? v.toString() : v), 2);

async function verify(agent: string, opts: ReadOptions, asJson: boolean): Promise<number> {
  const r = await verifyAgent(agent, opts);
  if (asJson) {
    console.log(json(r));
    return r.deal ? 0 : 2;
  }
  const v = r.certificate;
  if (!v) {
    console.log(`${agent}: no Bound certificate on ${r.contract}`);
  } else {
    console.log(`${agent}: ${describeCert(v)}`);
    console.log(
      `  coverage ${fmtUsdc(r.coverage)} USDC = reserve ${fmtUsdc(v.reserve_locked)} + auditor allocation ${fmtUsdc(v.allocation)} (the most arbitrated victims recover, shared by every creditor)`,
    );
  }
  const c = r.confinement;
  console.log(
    c
      ? `  confinement: ${c.confined ? "confined" : "NOT confined"} (${c.keys.length} key(s), ${describeCode(c.code)})`
      : "  confinement: unknown (account could not be read)",
  );
  if (v?.settlement) console.log(`  settled: ${JSON.stringify(v.settlement)}`);
  console.log(`  decision: ${r.deal ? "deal (Verified)" : `refuse (${r.reason})`}`);
  return r.deal ? 0 : 2;
}

async function cert(idText: string, opts: ReadOptions, asJson: boolean): Promise<number> {
  if (!/^\d+$/.test(idText)) throw new Error(`certificate id must be a number, got ${idText}`);
  const v: CertViewJson | null = await getCertificate(Number(idText), opts);
  if (asJson) {
    console.log(json(v));
    return v ? 0 : 2;
  }
  if (!v) {
    console.log(`no certificate ${idText}`);
    return 2;
  }
  console.log(describeCert(v));
  console.log(`  coverage ${fmtUsdc(coverageOf(v))} USDC, halted ${v.halted}`);
  if (v.settlement) console.log(`  settled: ${JSON.stringify(v.settlement)}`);
  return 0;
}

async function mcp(opts: ReadOptions): Promise<void> {
  const { StdioServerTransport } = await import("@modelcontextprotocol/sdk/server/stdio.js");
  const { createNearMcpServer } = await import("./mcp.js");
  const server = createNearMcpServer({
    rpc: opts.rpc,
    contractId: opts.contractId,
    agentId: process.env.BOUND_NEAR_AGENT_ID,
    agentKey: process.env.BOUND_NEAR_AGENT_KEY,
  });
  await server.connect(new StdioServerTransport());
}

async function main(): Promise<number> {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      contract: { type: "string" },
      rpc: { type: "string", multiple: true },
      json: { type: "boolean", default: false },
      help: { type: "boolean", short: "h", default: false },
      version: { type: "boolean", short: "v", default: false },
    },
  });
  if (values.version) {
    console.log(VERSION);
    return 0;
  }
  const [cmd, arg] = positionals;
  if (values.help || !cmd) {
    console.log(HELP);
    return values.help ? 0 : 1;
  }
  quietRpcRetryLogs();
  const opts: ReadOptions = {
    contractId: values.contract,
    rpc: values.rpc?.flatMap((u) => u.split(",")).filter(Boolean),
  };
  switch (cmd) {
    case "verify":
      if (!arg) throw new Error("usage: bound-near verify <agent account id>");
      return verify(arg, opts, values.json);
    case "cert":
      if (!arg) throw new Error("usage: bound-near cert <certificate id>");
      return cert(arg, opts, values.json);
    case "mcp":
      await mcp(opts);
      return -1;
    default:
      throw new Error(`unknown command ${cmd}; run bound-near --help`);
  }
}

main().then(
  (code) => {
    if (code >= 0) process.exit(code);
  },
  (e: unknown) => {
    console.error(`bound-near: ${e instanceof Error ? e.message : String(e)}`);
    process.exit(1);
  },
);
