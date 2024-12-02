import {
  PublicKey,
  Connection,
  Keypair,
  Transaction,
  SystemProgram,
  TransactionInstruction,
  SYSVAR_RENT_PUBKEY,
  ComputeBudgetProgram
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddress,
} from "@solana/spl-token";
import * as anchor from "@project-serum/anchor";
import BN from "bn.js";

// ---------------------- Constants ----------------------

const PROGRAM_ID = new PublicKey("4UjdrPr1Tv1974XZgLRZ63Wu4XisLRS2rh9K4ChK1wB7");
const TOKEN_MINT = new PublicKey("13WjtSt6dp9qQFrvcx1ncD2gHSyhNMAqwEqwQkSgpmya");
const connection = new Connection("https://api.devnet.solana.com", "confirmed");
const wallet = Keypair.fromSecretKey(
  Uint8Array.from(
    [PIRVATEKEYS]
  )
);

async function initializePresale() {
  // Derive the PDA for the presale account
  const [presalePda, bump] = await PublicKey.findProgramAddress(
    [Buffer.from("presale"), TOKEN_MINT.toBuffer()],
    PROGRAM_ID
  );

  // Get the associated token account for the presale wallet
  const presaleTokenAccount = await getAssociatedTokenAddress(
    TOKEN_MINT,
    presalePda,
    true
  );

  // Define the cliff timestamp
  const cliffTimestamp = Math.floor(new Date("2024-11-27T00:00:00Z").getTime() / 1000);

  // Precomputed discriminator for `initialize_presale`
  const discriminator = Buffer.from([0x91, 0x83, 0xfa, 0x88, 0x23, 0xe5, 0x6f, 0x03]);

  // Construct the instruction data
  const data = Buffer.concat([
    discriminator,
    TOKEN_MINT.toBuffer(),
    wallet.publicKey.toBuffer(),
    Buffer.from([bump]),
    new BN(1_000_000_000).toArrayLike(Buffer, "le", 8), // Public sale price
    new BN(100_000).toArrayLike(Buffer, "le", 8),       // Max tokens
    new BN(85_000_000_000).toArrayLike(Buffer, "le", 8), // Max SOL
    new BN(cliffTimestamp).toArrayLike(Buffer, "le", 8)  // Cliff timestamp
  ]);

  // Add a compute budget instruction to increase compute units
  const computeBudgetIx = ComputeBudgetProgram.setComputeUnitLimit({
    units: 400_000,
  });

  // Build the transaction instruction
  const instruction = new TransactionInstruction({
    keys: [
      { pubkey: presalePda, isSigner: false, isWritable: true },
      { pubkey: presaleTokenAccount, isSigner: false, isWritable: true },
      { pubkey: TOKEN_MINT, isSigner: false, isWritable: false },
      { pubkey: wallet.publicKey, isSigner: true, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: ASSOCIATED_TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false },
    ],
    programId: PROGRAM_ID,
    data: data,
  });

  // Build and send the transaction
  const transaction = new Transaction().add(computeBudgetIx, instruction);
  transaction.feePayer = wallet.publicKey;

  const { blockhash } = await connection.getLatestBlockhash();
  transaction.recentBlockhash = blockhash;

  // Sign and send the transaction
  transaction.partialSign(wallet);
  const txid = await connection.sendRawTransaction(transaction.serialize());
  console.log(`Transaction sent: ${txid}`);
  console.log(`Presale PDA: ${presalePda.toString()}`);
  console.log(`Presale Token Account: ${presaleTokenAccount.toString()}`);
}

initializePresale().catch(console.error);//test
