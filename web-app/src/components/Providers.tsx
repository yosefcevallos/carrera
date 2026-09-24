"use client";

import { useMemo, type ReactNode } from "react";
import { ConnectionProvider, WalletProvider } from "@solana/wallet-adapter-react";
import { WalletModalProvider } from "@solana/wallet-adapter-react-ui";
import { PhantomWalletAdapter, SolflareWalletAdapter } from "@solana/wallet-adapter-wallets";
import { RPC_URL } from "@/lib/chain/config";
import { PositionStoreProvider } from "@/store/position-provider";
import { UiStoreProvider } from "@/store/ui-provider";
import { VaultStoreProvider } from "@/store/vault-provider";
import { WalletStoreProvider } from "@/store/wallet-provider";
import PositionSyncer from "./PositionSyncer";
import VaultSyncer from "./VaultSyncer";
import WalletBridge from "./WalletBridge";
import "@solana/wallet-adapter-react-ui/styles.css";

export default function Providers({ children }: { children: ReactNode }) {
  // Backpack registers through Wallet Standard and is picked up automatically.
  const wallets = useMemo(() => [new PhantomWalletAdapter(), new SolflareWalletAdapter()], []);
  return (
    <ConnectionProvider endpoint={RPC_URL}>
      <WalletProvider wallets={wallets} autoConnect>
        <WalletModalProvider>
          <WalletStoreProvider>
            <VaultStoreProvider>
              <PositionStoreProvider>
                <UiStoreProvider>
                  <WalletBridge />
                  <VaultSyncer />
                  <PositionSyncer />
                  {children}
                </UiStoreProvider>
              </PositionStoreProvider>
            </VaultStoreProvider>
          </WalletStoreProvider>
        </WalletModalProvider>
      </WalletProvider>
    </ConnectionProvider>
  );
}
