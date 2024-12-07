import {
  PublicKey,
  Connection,
  Keypair,
  SystemProgram,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import * as anchor from "@project-serum/anchor";

// Replace with your Program ID and Token Mint Address
const PROGRAM_ID = new PublicKey("EehBgsqLEpn3cR17vZqYnQYzcFtDyiQaWmGJZwagNzED");
const TOKEN_MINT = new PublicKey("13WjtSt6dp9qQFrvcx1ncD2gHSyhNMAqwEqwQkSgpmya");

// Set up the connection and provider
const connection = new Connection("https://api.devnet.solana.com", "confirmed");

// Use the same Keypair for both admin and contributor wallets
const adminAndContributorKeypair = Keypair.fromSecretKey(
  Uint8Array.from([
    // Replace with the secret key for FAMWSk1En5dJkEQrzPf9N1WS5KbXRq6F8sUUJvWq4cL9
  ])
);

// Initialize wallet and provider
const wallet = new anchor.Wallet(adminAndContributorKeypair);
const provider = new anchor.AnchorProvider(connection, wallet, {
  preflightCommitment: "processed",
});
anchor.setProvider(provider);

// Load the Anchor program
const idl = require("./idl.json"); // Ensure this points to your IDL file
const program = new anchor.Program(idl, PROGRAM_ID, provider);

/**
 * Initializes the presale account with the given parameters.
 */
async function initializePresale() {
  // Derive the presale account PDA and bump
  const [presaleAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("presale"), adminAndContributorKeypair.publicKey.toBuffer(), TOKEN_MINT.toBuffer()],
    PROGRAM_ID
  );

  const admin = adminAndContributorKeypair.publicKey;

  // Define presale parameters
  const publicSalePrice = 1_000_000; // Price per token in lamports
  const maxTokens = 1_000_000; // Maximum tokens for presale
  const maxSol = 10_000_000; // Maximum SOL hard cap

  console.log("Presale Account (PDA):", presaleAccount.toBase58());

  // Call the `initializePresale` method
  await program.methods
    .initializePresale(TOKEN_MINT, admin, bump, publicSalePrice, maxTokens, maxSol)
    .accounts({
      presaleAccount: presaleAccount,
      admin: admin,
      tokenMint: TOKEN_MINT,
      systemProgram: SystemProgram.programId,
    })
    .signers([adminAndContributorKeypair]) // Admin must sign
    .rpc();

  console.log("Presale initialized successfully!");
}

/**
 * Contributes lamports to the presale and allocates tokens to the contributor.
 */
async function contributeToPresale(presaleAccountPubkey: PublicKey, lamportsPaid: number) {
  // Derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), adminAndContributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  console.log("Contributor Allocation Account (PDA):", allocationAccount.toBase58());

  // Call the `contribute` method
  await program.methods
    .contribute(new anchor.BN(lamportsPaid))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccount,
      contributor: adminAndContributorKeypair.publicKey,
      adminWallet: adminAndContributorKeypair.publicKey,
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .signers([adminAndContributorKeypair]) // Contributor must sign
    .rpc();

  console.log(`Contributed ${lamportsPaid} lamports to the presale.`);
}

/**
 * Claims tokens from the presale based on the vesting schedule.
 */
async function claimTokens(presaleAccountPubkey: PublicKey, presaleWalletPubkey: PublicKey, claimableNow: number) {
  // Derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), adminAndContributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  console.log(`Claiming ${claimableNow} tokens...`);

  // Call the `claimTokens` method
  await program.methods
    .claimTokens(new anchor.BN(claimableNow))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccount,
      presaleWallet: presaleWalletPubkey,
      contributorWallet: adminAndContributorKeypair.publicKey, // Admin and contributor wallets are the same
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .signers([adminAndContributorKeypair]) // Contributor must sign
    .rpc();

  console.log("Tokens claimed successfully!");
}

/**
 * Closes the presale.
 */
async function closePresale(presaleAccountPubkey: PublicKey) {
  console.log("Closing presale...");

  // Call the `closePresale` method
  await program.methods
    .closePresale()
    .accounts({
      presaleAccount: presaleAccountPubkey,
      admin: adminAndContributorKeypair.publicKey,
    })
    .signers([adminAndContributorKeypair]) // Admin must sign
    .rpc();

  console.log("Presale closed successfully!");
}

// ** Example Usage **
(async () => {
  try {
    console.log("Initializing presale...");
    await initializePresale();

    const [presaleAccount] = await PublicKey.findProgramAddress(
      [Buffer.from("presale"), adminAndContributorKeypair.publicKey.toBuffer(), TOKEN_MINT.toBuffer()],
      PROGRAM_ID
    );

    console.log("Contributing to presale...");
    await contributeToPresale(presaleAccount, 1_000_000); // Contribute 1,000,000 lamports

    console.log("Claiming tokens...");
    const presaleWallet = new PublicKey("REPLACE_WITH_PRESALE_WALLET_ADDRESS"); // Replace with your presale wallet address
    await claimTokens(presaleAccount, presaleWallet, 1000); // Claim 1000 tokens

    console.log("Closing presale...");
    await closePresale(presaleAccount);

    console.log("All operations completed successfully!");
  } catch (err) {
    console.error("Error:", err);
  }
})();
