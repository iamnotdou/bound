// Streaming chat endpoint. The operator (human) talks to the Bound agent here.
// Runs server-side — the agent's secret key never reaches the browser.
import { formatDataStreamPart } from "ai";
import { runBoundAgent } from "@/app/lib/agent";

export const runtime = "nodejs";
export const maxDuration = 60;

export async function POST(req: Request) {
  const { messages } = await req.json();

  // The one known setup gotcha: without the key the model call throws and the AI
  // SDK masks it as "An error occurred." Surface the real cause so the chat panel
  // can tell the user exactly what to fix.
  if (!process.env.ANTHROPIC_API_KEY) {
    return new Response(
      formatDataStreamPart(
        "error",
        "ANTHROPIC_API_KEY is not set — add it to .env.testnet to enable the chat.",
      ),
      { headers: { "content-type": "text/plain; charset=utf-8", "x-vercel-ai-data-stream": "v1" } },
    );
  }

  const result = runBoundAgent(messages);
  // Forward real error messages (instead of the SDK's masked default) so failures
  // are debuggable in the UI.
  return result.toDataStreamResponse({
    getErrorMessage: (e) => (e instanceof Error ? e.message : String(e)),
  });
}
