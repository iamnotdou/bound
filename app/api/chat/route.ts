// Streaming chat endpoint. The operator (human) talks to the Bound agent here.
// Runs server-side — the agent's secret key never reaches the browser.
import { runBoundAgent } from "@/app/lib/agent";

export const runtime = "nodejs";
export const maxDuration = 60;

export async function POST(req: Request) {
  const { messages } = await req.json();
  const result = runBoundAgent(messages);
  return result.toDataStreamResponse();
}
