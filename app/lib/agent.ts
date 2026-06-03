// AI SDK adapter — runs Claude in a tool-use loop over the Bound tools. Used by
// the /chat demo. Claude lives inside our app; the tools call BoundClient.
import { createAnthropic } from "@ai-sdk/anthropic";
import { streamText, tool, type CoreMessage } from "ai";
import { z } from "zod";
import { boundTools } from "./agent-tools";

const MODEL = process.env.ANTHROPIC_MODEL ?? "claude-sonnet-4-5";

// Provider points at whatever Anthropic-compatible endpoint ANTHROPIC_BASE_URL
// names. Default is the openllm.sh gateway (Anthropic format on /v1/messages);
// the SDK appends `/messages`, so the base URL ends in `/v1`. To talk to
// Anthropic directly, set ANTHROPIC_BASE_URL=https://api.anthropic.com/v1.
const anthropic = createAnthropic({
  apiKey: process.env.ANTHROPIC_API_KEY,
  baseURL: process.env.ANTHROPIC_BASE_URL ?? "https://openllm.sh/v1",
});

const SYSTEM = `You are an autonomous AI payment agent operating on the Stellar network through the Bound Protocol.

You hold a wallet and can pay for services on your own. You have NO spending cap — you pay what you are asked. What keeps your counterparties safe is your Bound Certificate: an auditor staked their own capital to vouch that your worst-case impact is bounded and pre-funded by a locked reserve. If that vouch is ever false, anyone can challenge it and the auditor is slashed on-chain.

Be concise. When you take an on-chain action, report the transaction result plainly. When you pay a service via x402, make clear that you decided to pay autonomously — no human approved the amount.`;

// Adapt the framework-agnostic tools into AI SDK tools.
const aiTools = Object.fromEntries(
  Object.entries(boundTools).map(([name, t]) => [
    name,
    tool({ description: t.description, parameters: z.object(t.parameters), execute: t.execute }),
  ]),
);

export function runBoundAgent(messages: CoreMessage[]) {
  return streamText({
    model: anthropic(MODEL),
    system: SYSTEM,
    messages,
    tools: aiTools,
    maxSteps: 8,
  });
}
