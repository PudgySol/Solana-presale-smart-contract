## Solana Presale Smart Contracts Overview

### Purpose
Presale and vesting contracts plus helper scripts for Solana-based token launches, mixing Anchor programs (`*.rs`), a Solidity vesting mock, and TypeScript deployment utilities.

### Key Components
- `Success_presale_w_Vesting.sol`: Anchor-style presale with vesting and allocation tracking.
- `Auth_Suc_PS.rs`: Advanced presale supporting SOL caps, refunds, airdrops, and price updates.
- `liquidity_yield.rs`, `Main_Presale.rs`, `Presale-Play.rs`, etc.: Variants covering liquidity, multiple vesting schedules, and gamified presales.
- `deployer.ts` / `Presl.ts`: Web3 scripts that drive PDA initialization and presale setup.

### Current Risks
1. `deployer.ts` embeds a private key literal, so committing the file compromises the referenced wallet.
2. `Success_presale_w_Vesting.sol` treats an allocation data account as an SPL token account inside `contribute`, causing the CPI transfer to fail.
3. `Auth_Suc_PS.rs` manipulates lamports on a `TokenAccount` during refunds, which will panic because SPL accounts do not carry lamport balances.

### Suggested Next Steps
- Externalize keys into a secure wallet loader or environment variable and regenerate the compromised keypair.
- Fix the Anchor account schemas so CPI transfers use real token accounts and contributor ATAs.
- Separate SOL accounting from SPL token custody in the refund logic and add integration tests for the presale lifecycle.
