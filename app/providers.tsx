"use client";
// Client-side providers wrapper, mounted once in the root layout.
import type { ReactNode } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { WalletProvider } from "@/app/lib/wallet/useWallet";

export function Providers({ children }: { children: ReactNode }) {
  return (
    <WalletProvider>
      <TooltipProvider delayDuration={150}>{children}</TooltipProvider>
    </WalletProvider>
  );
}
