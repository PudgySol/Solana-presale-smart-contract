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

// Admin wallet
const adminKeypair = Keypair.fromSecretKey(
  Uint8Array.from([
    // Replace with the secret key for FAMWSk1En5dJkEQrzPf9N1WS5KbXRq6F8sUUJvWq4cL9
  ])
);

// Load the program IDL
import idl from "./idl.json";
const program = new anchor.Program(idl, PROGRAM_ID);

/**
 * Helper function to send a transaction to the blockchain.
 */
async function sendTransaction(transaction: Transaction, signers: Keypair[]) {
  transaction.feePayer = signers[0].publicKey; // Use the first signer as the fee payer
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
    [Buffer.from("presale"), adminKeypair.publicKey.toBuffer(), TOKEN_MINT.toBuffer()],
    PROGRAM_ID
  );

  const transaction = new Transaction();

  const instruction = program.instruction.initializePresale(
    TOKEN_MINT,
    adminKeypair.publicKey,
    bump,
    new anchor.BN(1_000_000), // publicSalePrice
    new anchor.BN(1_000_000), // maxTokens
    new anchor.BN(10_000_000), // maxSol
    {
      accounts: {
        presaleAccount: presaleAccount,
        admin: adminKeypair.publicKey,
        tokenMint: TOKEN_MINT,
        systemProgram: SystemProgram.programId,
      },
    }
  );

  transaction.add(instruction);

  console.log("Initializing presale...");
  await sendTransaction(transaction, [adminKeypair]);
  console.log("Presale initialized successfully!");
}

/**
 * Allows anyone to contribute to the presale and dynamically creates an allocation account.
 */
async function contributeToPresale(presaleAccountPubkey: PublicKey, contributorKeypair: Keypair, lamportsPaid: number) {
  // Dynamically derive the contributor's allocation account PDA
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), contributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  const transaction = new Transaction();

  const instruction = program.instruction.contribute(
    new anchor.BN(lamportsPaid), // Amount of SOL contributed
    {
      accounts: {
        presaleAccount: presaleAccountPubkey,
        allocationAccount: allocationAccount,
        contributor: contributorKeypair.publicKey, // Contributor wallet
        adminWallet: adminKeypair.publicKey, // Admin wallet receiving SOL
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      },
    }
  );

  transaction.add(instruction);

  console.log(`Contributing ${lamportsPaid} lamports to presale...`);
  await sendTransaction(transaction, [contributorKeypair]); // Contributor wallet signs
  console.log("Contribution successful!");
}

/**
 * Allows contributors to claim tokens based on their allocation.
 */
async function claimTokens(presaleAccountPubkey: PublicKey, contributorKeypair: Keypair, presaleWalletPubkey: PublicKey, claimableNow: number) {
  const [allocationAccount, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("allocation"), contributorKeypair.publicKey.toBuffer()],
    PROGRAM_ID
  );

  const transaction = new Transaction();

  const instruction = program.instruction.claimTokens(
    new anchor.BN(claimableNow),
    {
      accounts: {
        presaleAccount: presaleAccountPubkey,
        allocationAccount: allocationAccount,
        presaleWallet: presaleWalletPubkey,
        contributorWallet: contributorKeypair.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      },
    }
  );

  transaction.add(instruction);

  console.log(`Claiming ${claimableNow} tokens...`);
  await sendTransaction(transaction, [contributorKeypair]); // Contributor signs the transaction
  console.log("Tokens claimed successfully!");
}

/**
 * Closes the presale.
 */
async function closePresale(presaleAccountPubkey: PublicKey) {
  const transaction = new Transaction();

  const instruction = program.instruction.closePresale(
    {
      accounts: {
        presaleAccount: presaleAccountPubkey,
        admin: adminKeypair.publicKey,
      },
    }
  );

  transaction.add(instruction);

  console.log("Closing presale...");
  await sendTransaction(transaction, [adminKeypair]);
  console.log("Presale closed successfully!");
}

// ** Example Workflow **
(async () => {
  try {
    console.log("Initializing presale...");
    await initializePresale();

    const [presaleAccount] = await PublicKey.findProgramAddress(
      [Buffer.from("presale"), adminKeypair.publicKey.toBuffer(), TOKEN_MINT.toBuffer()],
      PROGRAM_ID
    );

    // Contributor 1
    const contributorKeypair1 = Keypair.generate();
    console.log("Contributor 1 contributing...");
    await contributeToPresale(presaleAccount, contributorKeypair1, 1_500_000); // 1.5 SOL

    // Contributor 2
    const contributorKeypair2 = Keypair.generate();
    console.log("Contributor 2 contributing...");
    await contributeToPresale(presaleAccount, contributorKeypair2, 2_500_000); // 2.5 SOL

    // Contributor 1 claims tokens
    const presaleWallet = new PublicKey("REPLACE_WITH_PRESALE_WALLET_ADDRESS");
    console.log("Contributor 1 claiming tokens...");
    await claimTokens(presaleAccount, contributorKeypair1, presaleWallet, 1000);

    console.log("Closing presale...");
    await closePresale(presaleAccount);

    console.log("All operations completed successfully!");
  } catch (err) {
    console.error("Error:", err);
  }
})();
