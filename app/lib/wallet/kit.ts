"use client";
// Stellar Wallets Kit v2.x is an all-static API. We init it once (browser only)
// with the hot-wallet modules a judge is likely to have. Each wallet module is
// its own subpath import. Connect via StellarWalletsKit.authModal().
import { StellarWalletsKit, Networks } from "@creit.tech/stellar-wallets-kit";
import { FreighterModule, FREIGHTER_ID } from "@creit.tech/stellar-wallets-kit/modules/freighter";
import { xBullModule } from "@creit.tech/stellar-wallets-kit/modules/xbull";
import { LobstrModule } from "@creit.tech/stellar-wallets-kit/modules/lobstr";
import { AlbedoModule } from "@creit.tech/stellar-wallets-kit/modules/albedo";
import { HanaModule } from "@creit.tech/stellar-wallets-kit/modules/hana";
import { RabetModule } from "@creit.tech/stellar-wallets-kit/modules/rabet";
import { NETWORK } from "../ui-config";

let _inited = false;

/** Idempotently initialize the kit; returns the static class for chained calls. */
export function ensureKit(): typeof StellarWalletsKit {
  if (!_inited) {
    StellarWalletsKit.init({
      network: NETWORK === "public" ? Networks.PUBLIC : Networks.TESTNET,
      selectedWalletId: FREIGHTER_ID,
      modules: [
        new FreighterModule(),
        new xBullModule(),
        new LobstrModule(),
        new AlbedoModule(),
        new HanaModule(),
        new RabetModule(),
      ],
    });
    _inited = true;
  }
  return StellarWalletsKit;
}
