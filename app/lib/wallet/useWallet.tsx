"use client";
// Wallet connection state + signing, shared app-wide. A connected wallet lets a
// judge play any open actor role. Signing is envelope-only: each role is both
// the tx source and the require_auth address, so one wallet signature satisfies
// Soroban auth. The unsigned XDR is built server-side (see /api/tx/build).
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { ensureKit } from "./kit";
import { network } from "../ui-config";

const LS_KEY = "bound:connected";

type WalletCtx = {
  address: string | null;
  connecting: boolean;
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;
  /** Sign an unsigned (assembled) XDR envelope with the connected wallet. */
  signXdr: (xdr: string) => Promise<string>;
};

const Ctx = createContext<WalletCtx | null>(null);

export function WalletProvider({ children }: { children: ReactNode }) {
  const [address, setAddress] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  // Best-effort silent restore: the kit persists the selected module, so
  // getAddress() returns the address again if a session is still live.
  useEffect(() => {
    if (typeof window === "undefined") return;
    if (!localStorage.getItem(LS_KEY)) return;
    (async () => {
      try {
        const { address } = await ensureKit().getAddress();
        if (address) setAddress(address);
        else localStorage.removeItem(LS_KEY);
      } catch {
        localStorage.removeItem(LS_KEY);
      }
    })();
  }, []);

  const connect = useCallback(async () => {
    setConnecting(true);
    try {
      const { address } = await ensureKit().authModal();
      setAddress(address);
      if (typeof window !== "undefined") localStorage.setItem(LS_KEY, "1");
    } finally {
      setConnecting(false);
    }
  }, []);

  const disconnect = useCallback(async () => {
    setAddress(null);
    if (typeof window !== "undefined") localStorage.removeItem(LS_KEY);
    try {
      await ensureKit().disconnect();
    } catch {
      /* module without disconnect — clearing local state is enough */
    }
  }, []);

  const signXdr = useCallback(
    async (xdr: string) => {
      const { signedTxXdr } = await ensureKit().signTransaction(xdr, {
        address: address ?? undefined,
        networkPassphrase: network.passphrase,
      });
      return signedTxXdr;
    },
    [address],
  );

  return (
    <Ctx.Provider value={{ address, connecting, connect, disconnect, signXdr }}>
      {children}
    </Ctx.Provider>
  );
}

export function useWallet(): WalletCtx {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useWallet must be used within <WalletProvider>");
  return ctx;
}
