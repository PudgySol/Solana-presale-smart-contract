import {
  PublicKey,
  Connection,
  Keypair,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import * as anchor from "@project-serum/anchor";

// Replace with your Program ID and Token Mint Address
const PROGRAM_ID = new PublicKey("EehBgsqLEpn3cR17vZqYnQYzcFtDyiQaWmGJZwagNzED");
const TOKEN_MINT = new PublicKey("13WjtSt6dp9qQFrvcx1ncD2gHSyhNMAqwEqwQkSgpmya");

// Set up the connection
const connection = new Connection("https://api.devnet.solana.com", "confirmed");

// Load your Keypair for admin and contributor (same wallet)
const adminAndContributorKeypair = Keypair.fromSecretKey(
  Uint8Array.from([
    // Replace with the secret key for FAMWSk1En5dJkEQrzPf9N1WS5KbXRq6F8sUUJvWq4cL9
  ])
);

// Load the program IDL
const idl = require("./idl.json");
const program = new anchor.Program(idl, PROGRAM_ID);

/**
 * Helper function to send a transaction to the blockchain.
 */
async function sendTransaction(transaction: Transaction, signers: Keypair[]) {
  transaction.feePayer = adminAndContributorKeypair.publicKey;
  transaction.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  await transaction.sign(...signers);
  const txId = await sendAndConfirmTransaction(connection, transaction, signers);
  console.log("Transaction successful with ID:", txId);
  return txId;
}

/**
 * Initializes the presale account with the given parameters.
 */
async function initializePresale() {
  const [presaleAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("presale"), adminAndContributorKeypair.publicKey.toBuffer(), TOKEN_MINT.toBuffer()],
    PROGRAM_ID
  );

  const transaction = new Transaction();

  // Program instruction to initialize the presale
  const instruction = program.instruction.initializePresale(
    TOKEN_MINT,
    adminAndContributorKeypair.publicKey,
    bump,
    new anchor.BN(1_000_000), // publicSalePrice
    new anchor.BN(1_000_000), // maxTokens
    new anchor.BN(10_000_000), // maxSol
    {
      accounts: {
        presaleAccount: presaleAccount,
        admin: adminAndContributorKeypair.publicKey,
        tokenMint: TOKEN_MINT,
        systemProgram: SystemProgram.programId,
      },
    }
  );

  transaction.add(instruction);

  console.log("Initializing presale...");
  await sendTransaction(transaction, [adminAndContributorKeypair]);
  console.log("Presale initialized successfully!");
}

/**
 * Contributes lamports to the presale and allocates tokens to the contributor.
 */
async function contributeToPresale(presaleAccountPubkey: PublicKey, lamportsPaid: number) {
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), adminAndContributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  const transaction = new Transaction();

  // Program instruction to contribute to the presale
  const instruction = program.instruction.contribute(
    new anchor.BN(lamportsPaid),
    {
      accounts: {
        presaleAccount: presaleAccountPubkey,
        allocationAccount: allocationAccount,
        contributor: adminAndContributorKeypair.publicKey,
        adminWallet: adminAndContributorKeypair.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      },
    }
  );

  transaction.add(instruction);

  console.log("Contributing to presale...");
  await sendTransaction(transaction, [adminAndContributorKeypair]);
  console.log(`Contributed ${lamportsPaid} lamports successfully!`);
}

/**
 * Claims tokens from the presale based on the vesting schedule.
 */
async function claimTokens(presaleAccountPubkey: PublicKey, presaleWalletPubkey: PublicKey, claimableNow: number) {
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), adminAndContributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  const transaction = new Transaction();

  // Program instruction to claim tokens
  const instruction = program.instruction.claimTokens(
    new anchor.BN(claimableNow),
    {
      accounts: {
        presaleAccount: presaleAccountPubkey,
        allocationAccount: allocationAccount,
        presaleWallet: presaleWalletPubkey,
        contributorWallet: adminAndContributorKeypair.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      },
    }
  );

  transaction.add(instruction);

  console.log(`Claiming ${claimableNow} tokens...`);
  await sendTransaction(transaction, [adminAndContributorKeypair]);
  console.log("Tokens claimed successfully!");
}

/**
 * Closes the presale.
 */
async function closePresale(presaleAccountPubkey: PublicKey) {
  const transaction = new Transaction();

  // Program instruction to close the presale
  const instruction = program.instruction.closePresale(
    {
      accounts: {
        presaleAccount: presaleAccountPubkey,
        admin: adminAndContributorKeypair.publicKey,
      },
    }
  );

  transaction.add(instruction);

  console.log("Closing presale...");
  await sendTransaction(transaction, [adminAndContributorKeypair]);
  console.log("Presale closed successfully!");
}

// ** Example Workflow **
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
