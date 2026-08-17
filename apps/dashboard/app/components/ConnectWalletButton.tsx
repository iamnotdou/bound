"use client";
import { Wallet, LogOut, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useWallet } from "@/app/lib/wallet/useWallet";
import { truncate, roleForAddress } from "@/app/lib/ui-config";

export function ConnectWalletButton() {
  const { address, connecting, connect, disconnect } = useWallet();

  if (!address) {
    return (
      <Button size="sm" onClick={connect} disabled={connecting}>
        <Wallet className="size-4" />
        {connecting ? "Connecting…" : "Connect wallet"}
      </Button>
    );
  }

  const role = roleForAddress(address);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button size="sm" variant="outline" className="font-mono">
          <Wallet className="size-4" />
          {truncate(address)}
          {role && <span className="font-sans text-muted-foreground">· {role}</span>}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuLabel className="font-mono text-xs break-all">{address}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={() => navigator.clipboard.writeText(address).catch(() => {})}>
          <Copy className="size-4" /> Copy address
        </DropdownMenuItem>
        <DropdownMenuItem onClick={disconnect} variant="destructive">
          <LogOut className="size-4" /> Disconnect
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
