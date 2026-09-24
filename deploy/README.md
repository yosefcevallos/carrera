# deploy

Mainnet / devnet deployment and initialisation for `carrera_overlay`.

1. Build: `cd ../program && PATH=~/.local/share/solana/install/releases/3.1.1/solana-release/bin:$PATH anchor build -- --features mock-venues`
2. Deploy: `solana program deploy ../program/target/deploy/carrera_overlay.so --program-id ../program/target/deploy/carrera_overlay-keypair.json -u <rpc> --with-compute-unit-price 50000`
3. Initialise (idempotent): `ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/solana/id.json KEEPER_PUBKEY=<keeper> CAP_USD=25000 pnpm init`
   Prints a `VAULTS_JSON` line with every PDA; feed it to `supabase/` (`vaults` table) and to the keeper and web app configs.
4. `set_market_open` is sent by the keeper on its first pass.

`vaults.json` holds the verified mainnet mints and tiers. `params.ts` holds the spec §6.1 / §11 parameters.
