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

// Using the same wallet for both admin and contributor
const adminKeypair = Keypair.fromSecretKey(
  Uint8Array.from(
      //KeyPair Private Key
  )
);

// Load the program IDL
const idl = {/*IDL*/};
const program = new anchor.Program(idl as anchor.Idl, PROGRAM_ID);

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

  console.log("Presale Account PDA:", presaleAccount.toBase58());
  console.log("Bump Seed:", bump);

  // Set the start and end timestamps for the presale
  const currentTimestamp = Math.floor(Date.now() / 1000); // Current time in seconds
  const startTimestamp = currentTimestamp + 60; // Presale starts in 1 minute
  const endTimestamp = currentTimestamp + 3600; // Presale ends in 1 hour

  const transaction = new Transaction();

  const instruction = program.instruction.initializePresale(
    TOKEN_MINT,
    adminKeypair.publicKey,
    bump,
    new anchor.BN(1_000_000), // publicSalePrice
    new anchor.BN(1_000_000), // maxTokens
    new anchor.BN(10_000_000), // maxSol
    new anchor.BN(startTimestamp), // Start timestamp
    new anchor.BN(endTimestamp), // End timestamp
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

  console.log("Sending transaction...");
  await sendTransaction(transaction, [adminKeypair]);
  console.log(`Presale initialized successfully! Start: ${startTimestamp}, End: ${endTimestamp}`);
};

/**
 * Allows anyone to contribute to the presale and dynamically creates an allocation account.
 */
async function contributeToPresale(presaleAccountPubkey: PublicKey, contributorKeypair: Keypair, lamportsPaid: number) {
  const presaleAccountInfo = await program.account.presaleAccount.fetch(presaleAccountPubkey);

  const currentTimestamp = Math.floor(Date.now() / 1000);

  if (currentTimestamp < presaleAccountInfo.startTimestamp.toNumber()) {
    throw new Error("Presale has not started yet.");
  }

  if (currentTimestamp > presaleAccountInfo.endTimestamp.toNumber()) {
    throw new Error("Presale has already ended.");
  }

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
};

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

              
(async () => {
  try {
    console.log("Initializing presale...");
    await initializePresale();

    const [presaleAccount] = await PublicKey.findProgramAddress(
      [Buffer.from("presale"), adminKeypair.publicKey.toBuffer(), TOKEN_MINT.toBuffer()],
      PROGRAM_ID
    );
    
    console.log("Closing presale...");
    await closePresale(presaleAccount);

    console.log("All operations completed successfully!");
  } catch (err) {
    console.error("Error:", err);
  }
})();
