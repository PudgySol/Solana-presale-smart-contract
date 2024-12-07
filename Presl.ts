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

// Set up the provider and connection
const connection = new Connection("https://api.devnet.solana.com", "confirmed");
const wallet = anchor.Wallet.local();
const provider = new anchor.AnchorProvider(connection, wallet, {});
anchor.setProvider(provider);

// Load the program
const idl = require("./idl.json"); // Ensure this points to your IDL file
const program = new anchor.Program(idl, PROGRAM_ID, provider);

// ** Main Functions **

/**
 * Initializes the presale account with the given parameters.
 */
async function initializePresale(adminKeypair: Keypair) {
  // Derive the presale account PDA and bump
  const [presaleAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("presale")],
    PROGRAM_ID
  );

  const admin = adminKeypair.publicKey;

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
      systemProgram: SystemProgram.programId,
    })
    .signers([adminKeypair]) // Admin must sign
    .rpc();

  console.log("Presale initialized successfully!");
}

/**
 * Contributes lamports to the presale and allocates tokens to the contributor.
 */
async function contributeToPresale(
  presaleAccountPubkey: PublicKey,
  contributorKeypair: Keypair,
  adminWalletPubkey: PublicKey,
  lamportsPaid: number
) {
  // Derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), contributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  console.log("Contributor Allocation Account (PDA):", allocationAccount.toBase58());

  // Call the `contribute` method
  await program.methods
    .contribute(new anchor.BN(lamportsPaid))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccount,
      contributor: contributorKeypair.publicKey,
      adminWallet: adminWalletPubkey,
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .signers([contributorKeypair]) // Contributor must sign
    .rpc();

  console.log(`Contributed ${lamportsPaid} lamports to the presale.`);
}

/**
 * Claims tokens from the presale based on the vesting schedule.
 */
async function claimTokens(
  presaleAccountPubkey: PublicKey,
  contributorKeypair: Keypair,
  presaleWalletPubkey: PublicKey,
  contributorWalletPubkey: PublicKey,
  claimableNow: number
) {
  // Derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), contributorKeypair.publicKey.toBuffer()],
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
      contributorWallet: contributorWalletPubkey,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .signers([contributorKeypair]) // Contributor must sign
    .rpc();

  console.log("Tokens claimed successfully!");
}

/**
 * Automates token vesting based on the schedule.
 */
async function airdropTokens(
  presaleAccountPubkey: PublicKey,
  contributorWalletPubkey: PublicKey,
  presaleWalletPubkey: PublicKey
) {
  // Derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), contributorWalletPubkey.toBuffer()],
    PROGRAM_ID
  );

  console.log("Processing token vesting...");

  // Call the `airdropTokens` method
  await program.methods
    .airdropTokens()
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccount,
      presaleWallet: presaleWalletPubkey,
      contributorWallet: contributorWalletPubkey,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .rpc();

  console.log("Token airdrop completed!");
}

/**
 * Updates the public sale price of the tokens.
 */
async function updatePresalePrice(
  presaleAccountPubkey: PublicKey,
  adminKeypair: Keypair,
  newPrice: number
) {
  console.log(`Updating public sale price to ${newPrice} lamports...`);

  // Call the `updatePresalePrice` method
  await program.methods
    .updatePresalePrice(new anchor.BN(newPrice))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      admin: adminKeypair.publicKey,
    })
    .signers([adminKeypair]) // Admin must sign
    .rpc();

  console.log("Presale price updated successfully!");
}

/**
 * Issues a refund for tokens before vesting begins.
 */
async function refundTokens(
  presaleAccountPubkey: PublicKey,
  contributorKeypair: Keypair,
  presaleWalletPubkey: PublicKey,
  contributorWalletPubkey: PublicKey,
  tokenAmount: number
) {
  // Derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), contributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  console.log(`Requesting refund for ${tokenAmount} tokens...`);

  // Call the `refundTokens` method
  await program.methods
    .refundTokens(new anchor.BN(tokenAmount))
    .accounts({
      presaleAccount: presaleAccountPubkey,
      allocationAccount: allocationAccount,
      presaleWallet: presaleWalletPubkey,
      contributorWallet: contributorWalletPubkey,
      contributor: contributorKeypair.publicKey,
      tokenProgram: TOKEN_PROGRAM_ID,
    })
    .signers([contributorKeypair]) // Contributor must sign
    .rpc();

  console.log("Refund completed successfully!");
}

/**
 * Closes the presale.
 */
async function closePresale(presaleAccountPubkey: PublicKey, adminKeypair: Keypair) {
  console.log("Closing presale...");

  // Call the `closePresale` method
  await program.methods
    .closePresale()
    .accounts({
      presaleAccount: presaleAccountPubkey,
      admin: adminKeypair.publicKey,
    })
    .signers([adminKeypair]) // Admin must sign
    .rpc();

  console.log("Presale closed successfully!");
}

// ** Example Usage **
(async () => {
  try {
    const adminKeypair = Keypair.generate(); // Replace with your admin keypair
    const contributorKeypair = Keypair.generate(); // Replace with your contributor keypair

    console.log("Initializing presale...");
    await initializePresale(adminKeypair);

    const [presaleAccount] = await PublicKey.findProgramAddress(
      [Buffer.from("presale")],
      PROGRAM_ID
    );

    console.log("Contributing to presale...");
    await contributeToPresale(
      presaleAccount,
      contributorKeypair,
      adminKeypair.publicKey,
      1_000_000
    );

    console.log("Done!");
  } catch (err) {
    console.error("Error:", err);
  }
})();
