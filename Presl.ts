import * as anchor from "@project-serum/anchor";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";

// Replace with your Program ID and token mint address
const PROGRAM_ID = new PublicKey("YOUR_PROGRAM_ID"); // Replace with your deployed Program ID
const TOKEN_MINT = new PublicKey("YOUR_TOKEN_MINT_ADDRESS"); // Replace with your token mint address

// Set up the provider
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);

// Load the program
const idl = require("./idl.json"); // Replace with your IDL file
const program = new anchor.Program(idl, PROGRAM_ID, provider);

// Main Functions
async function initializePresale(adminKeypair: Keypair) {
  const presaleAccount = Keypair.generate(); // Generate a new presale account
  const admin = adminKeypair.publicKey;

  const bump = 0; // PDA bump (use program-derived address in production)
  const publicSalePrice = 1_000_000; // Example: 1 token = 1 lamport
  const maxTokens = 1_000_000; // Max tokens for the presale
  const maxSol = 10_000_000; // Max SOL hard cap

  console.log("Presale Account:", presaleAccount.publicKey.toBase58());

  await program.methods
    .initializePresale(TOKEN_MINT, admin, bump, publicSalePrice, maxTokens, maxSol)
    .accounts({
      presaleAccount: presaleAccount.publicKey,
      admin: admin,
      systemProgram: SystemProgram.programId,
    })
    .signers([adminKeypair, presaleAccount]) // Admin and presale account need to sign
    .rpc();

  console.log("Presale Initialized!");
}

async function contributeToPresale(
  presaleAccountPubkey: PublicKey,
  contributorKeypair: Keypair,
  adminWalletPubkey: PublicKey,
  lamportsPaid: number
) {
  const allocationAccount = Keypair.generate(); // Generate contributor's allocation account
  const tokenProgram = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

  console.log("Contributor Allocation Account:", allocationAccount.publicKey.toBase58());

  await program.methods
    .contribute(new anchor.BN(lamportsPaid))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccount.publicKey,
      contributor: contributorKeypair.publicKey,
      adminWallet: adminWalletPubkey,
      tokenProgram: tokenProgram,
      systemProgram: SystemProgram.programId,
    })
    .signers([contributorKeypair, allocationAccount]) // Contributor and allocation account must sign
    .rpc();

  console.log(`Contributed ${lamportsPaid} lamports to the presale!`);
}

async function claimTokens(
  presaleAccountPubkey: PublicKey,
  allocationAccountPubkey: PublicKey,
  presaleWalletPubkey: PublicKey,
  contributorWalletPubkey: PublicKey,
  contributorKeypair: Keypair,
  claimableNow: number
) {
  const tokenProgram = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

  console.log(`Claiming ${claimableNow} tokens...`);

  await program.methods
    .claimTokens(new anchor.BN(claimableNow))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccountPubkey,
      presaleWallet: presaleWalletPubkey,
      contributorWallet: contributorWalletPubkey,
      tokenProgram: tokenProgram,
    })
    .signers([contributorKeypair]) // Contributor must sign
    .rpc();

  console.log("Tokens claimed!");
}

async function airdropTokens(
  presaleAccountPubkey: PublicKey,
  allocationAccountPubkey: PublicKey,
  presaleWalletPubkey: PublicKey,
  contributorWalletPubkey: PublicKey
) {
  const tokenProgram = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

  console.log("Processing token vesting...");

  await program.methods
    .airdropTokens()
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccountPubkey,
      presaleWallet: presaleWalletPubkey,
      contributorWallet: contributorWalletPubkey,
      tokenProgram: tokenProgram,
    })
    .rpc();

  console.log("Airdrop completed!");
}

async function updatePresalePrice(
  presaleAccountPubkey: PublicKey,
  adminKeypair: Keypair,
  newPrice: number
) {
  console.log(`Updating public sale price to ${newPrice}...`);

  await program.methods
    .updatePresalePrice(new anchor.BN(newPrice))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      admin: adminKeypair.publicKey,
    })
    .signers([adminKeypair]) // Admin must sign
    .rpc();

  console.log("Presale price updated!");
}

async function refundTokens(
  presaleAccountPubkey: PublicKey,
  allocationAccountPubkey: PublicKey,
  presaleWalletPubkey: PublicKey,
  contributorWalletPubkey: PublicKey,
  contributorKeypair: Keypair,
  tokenAmount: number
) {
  const tokenProgram = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

  console.log(`Requesting refund for ${tokenAmount} tokens...`);

  await program.methods
    .refundTokens(new anchor.BN(tokenAmount))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccountPubkey,
      presaleWallet: presaleWalletPubkey,
      contributorWallet: contributorWalletPubkey,
      contributor: contributorKeypair.publicKey,
      tokenProgram: tokenProgram,
    })
    .signers([contributorKeypair]) // Contributor must sign
    .rpc();

  console.log("Refund completed!");
}

async function closePresale(
  presaleAccountPubkey: PublicKey,
  adminKeypair: Keypair
) {
  console.log("Closing presale...");

  await program.methods
    .closePresale()
    .accounts({
      presaleAccount: presaleAccountPubkey,
      admin: adminKeypair.publicKey,
    })
    .signers([adminKeypair]) // Admin must sign
    .rpc();

  console.log("Presale closed!");
}

// Example Usage
(async () => {
  const adminKeypair = Keypair.generate(); // Replace with your admin keypair
  const contributorKeypair = Keypair.generate(); // Replace with your contributor keypair

  // Example Presale Account PublicKey (replace after initializing)
  const presaleAccountPubkey = new PublicKey("REPLACE_WITH_PRESALE_ACCOUNT");

  // Example Usage of Functions
  await initializePresale(adminKeypair);
  await contributeToPresale(presaleAccountPubkey, contributorKeypair, adminKeypair.publicKey, 1000000);
  await claimTokens(presaleAccountPubkey, new PublicKey("ALLOCATION_ACCOUNT"), new PublicKey("PRESALE_WALLET"), new PublicKey("CONTRIBUTOR_WALLET"), contributorKeypair, 1000);
})();
