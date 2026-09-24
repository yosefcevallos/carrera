// Ops cranks from the dashboard. Mock mode mutates the mock keeper world; rpc mode builds
// the instruction and asks the connected (keeper or guardian) wallet to sign.
import { Transaction } from "@solana/web3.js";
import type { Ticker } from "@/constants/vaults";
import { mockOpsAction, type OpsActionKind } from "@/lib/mock/ops";
import type { Signer } from "./actions";
import { DATA_SOURCE } from "./config";

export type { OpsActionKind };

export const OPS_ACTION_LABEL: Record<OpsActionKind, string> = {
  rebalance_to_kamino: "Rebalance to Kamino",
  rebalance_to_phoenix: "Rebalance to Phoenix",
  unwind_emergency: "Unwind (emergency)",
  pause: "Pause",
};

/** Returns the toast text. Throws with a plain-language message on refusal. */
export async function opsAction(kind: OpsActionKind, ticker: Ticker, signer?: Signer): Promise<string> {
  if (DATA_SOURCE === "mock") return mockOpsAction(kind, ticker);
  if (!signer) throw new Error("Connect the keeper or guardian wallet first.");
  const [ix, { XSTOCK_MINTS }] = await Promise.all([import("./ix"), import("./mints")]);
  const k = ix.vaultKeys(XSTOCK_MINTS[ticker]);
  const instruction =
    kind === "rebalance_to_kamino"
      ? ix.rebalanceToKaminoIx(signer.publicKey, k)
      : kind === "rebalance_to_phoenix"
        ? ix.rebalanceToPhoenixIx(signer.publicKey, k)
        : kind === "unwind_emergency"
          ? ix.unwindStartIx(signer.publicKey, k, ix.UNWIND_EMERGENCY)
          : ix.pauseIx(signer.publicKey, k.registry);
  const sig = await signer.sendTransaction(new Transaction().add(instruction));
  return `${OPS_ACTION_LABEL[kind]} sent for ${ticker}. ${sig.slice(0, 8)}…`;
}
