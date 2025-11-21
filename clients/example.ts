/**
 * Comprehensive example testing all instructions using the Codama-generated UMI client
 *
 * This script tests:
 * 1. initialize - Initialize the global config
 * 2. create - Create a new token and bonding curve
 * 3. buy - Buy tokens (first buy and subsequent buys)
 * 4. sell - Sell tokens back to the curve
 * 5. updateGlobalConfig - Update the global configuration
 * 6. withdraw - Withdraw funds after graduation
 */

import { createUmi } from "@metaplex-foundation/umi-bundle-defaults";
import {
  TransactionBuilderSendAndConfirmOptions,
  generateSigner,
  keypairIdentity,
  sol,
  publicKey,
  PublicKey,
  transactionBuilder,
  createSignerFromKeypair,
  Keypair,
} from "@metaplex-foundation/umi";
import {
  publicKey as publicKeySerializer,
  string,
  bytes,
} from "@metaplex-foundation/umi/serializers";
import { getCoinfunProgramId } from "./generated/umi/src/programs/coinfun";
import { createCoinfunProgram } from "./generated/umi/src/programs/coinfun";
import {
  initialize,
  create,
  buy,
  sell,
  updateGlobalConfig,
  withdraw,
  withdrawReserve,
  depositToReserve,
} from "./generated/umi/src/instructions";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { Keypair as SolanaKeypair } from "@solana/web3.js";
import "dotenv/config";

const umi = createUmi("http://127.0.0.1:8899", { commitment: "processed" });
//const umi = createUmi("https://api.devnet.solana.com", {
//  commitment: "processed",
//});

// ========================================
// Register custom program ID
// ========================================
// Set your program ID here or via environment variable COINFUN_PROGRAM_ID
// The program ID should match the deployed program address
const COINFUN_PROGRAM_ID =
  process.env.COINFUN_PROGRAM_ID ||
  "6xHUH1pJ2sLCm8YwyF8DBkUyBjDvoJKWgaXiBLaDdgsp"; // Default from generated client
console.log(`Program id: ${COINFUN_PROGRAM_ID}`);

// Register the program with UMI so all instructions use this program ID
const customProgram = createCoinfunProgram();
customProgram.publicKey = publicKey(COINFUN_PROGRAM_ID);
umi.programs.add(customProgram);

console.log(`📌 Using program ID: ${COINFUN_PROGRAM_ID}\n`);

// Load authority keypair from file
const keypairPath = path.join(os.homedir(), ".config", "solana", "id.json");
const keypairFile = fs.readFileSync(keypairPath, "utf-8");
const keypairArray = JSON.parse(keypairFile);
const solanaKeypair = SolanaKeypair.fromSecretKey(new Uint8Array(keypairArray));
const authorityKeypair: Keypair = {
  publicKey: publicKey(solanaKeypair.publicKey.toBase58()),
  secretKey: solanaKeypair.secretKey,
};
const authority = createSignerFromKeypair(umi, authorityKeypair);

// Generate other keypairs
const creator = generateSigner(umi);
const buyer = generateSigner(umi);

// Fee recipient is the same as authority
const feeRecipient = authority;

umi.use(keypairIdentity(authority));

const options: TransactionBuilderSendAndConfirmOptions = {
  confirm: { commitment: "processed" },
};

// Constants
const LAMPORTS_PER_SOL = BigInt(1_000_000_000);
const initialVirtualTokenReserves = BigInt(1_073_000_191_000_000); // 1_073_000_191 * 1e6
const initialVirtualSolReserves = BigInt(30) * LAMPORTS_PER_SOL;
const tokenTotalSupply = BigInt(1_000_000_000_000_000); // 1_000_000_000 * 1e6
const platformTradeFeeBps = BigInt(100); // 1%
const reserveTradeFeeBps = BigInt(400); // 4%
const graduationThreshold = BigInt(85) * LAMPORTS_PER_SOL;

// Token program address
const TOKEN_PROGRAM_ID = publicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
);
const ASSOCIATED_TOKEN_PROGRAM_ID = publicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
);
const TOKEN_METADATA_PROGRAM_ID = publicKey(
  "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
);

// Helper function to derive PDA
function findPda(seeds: Uint8Array[]): PublicKey {
  const [pda] = umi.eddsa.findPda(getCoinfunProgramId(umi), seeds);
  return pda;
}

// Helper function to get associated token address using UMI
function getAssociatedTokenAddress(
  mint: PublicKey,
  owner: PublicKey
): PublicKey {
  const [ata] = umi.eddsa.findPda(ASSOCIATED_TOKEN_PROGRAM_ID, [
    publicKeySerializer().serialize(owner),
    publicKeySerializer().serialize(TOKEN_PROGRAM_ID),
    publicKeySerializer().serialize(mint),
  ]);
  return ata;
}

// Helper function to get metadata PDA
function getMetadataPda(mint: PublicKey): PublicKey {
  const [metadataPda] = umi.eddsa.findPda(TOKEN_METADATA_PROGRAM_ID, [
    bytes().serialize(new Uint8Array([109, 101, 116, 97, 100, 97, 116, 97])), // "metadata"
    publicKeySerializer().serialize(TOKEN_METADATA_PROGRAM_ID),
    publicKeySerializer().serialize(mint),
  ]);
  return metadataPda;
}

async function main() {
  console.log("🚀 Starting comprehensive CoinFun client test...\n");

  // ========================================
  // 1. AIRDROP FUNDS
  // ========================================
  console.log("📦 Airdropping funds to keypairs...");
  try {
    await umi.rpc.airdrop(authority.publicKey, sol(20), options.confirm);
    console.log(`   ✅ Airdropped 20 SOL to authority: ${authority.publicKey}`);

    await umi.rpc.airdrop(creator.publicKey, sol(10), options.confirm);
    console.log(`   ✅ Airdropped 10 SOL to creator: ${creator.publicKey}`);

    await umi.rpc.airdrop(buyer.publicKey, sol(10), options.confirm);
    console.log(`   ✅ Airdropped 10 SOL to buyer: ${buyer.publicKey}`);

    // Fee recipient is the same as authority, no need to airdrop separately
    console.log(
      `   ✅ Fee recipient is authority: ${feeRecipient.publicKey}\n`
    );
  } catch (error) {
    console.error("   ❌ Error airdropping funds:", error);
    throw error;
  }

  // ========================================
  // 2. INITIALIZE
  // ========================================
  console.log("1️⃣  Testing initialize instruction...");
  try {
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);
    console.log(`   Global PDA: ${globalPda.toString()}`);
    console.log(`   Global Reserve PDA: ${globalReservePda.toString()}`);

    await initialize(umi, {
      authority,
      initialVirtualTokenReserves,
      initialVirtualSolReserves,
      tokenTotalSupply,
      platformTradeFeeBps,
      reserveTradeFeeBps,
      platformFeeRecipient: feeRecipient.publicKey,
      graduationThreshold,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Initialize successful!\n`);
  } catch (error) {
    console.error(`   ❌ Initialize failed:`, error);
    throw error;
  }

  // ========================================
  // 3. CREATE
  // ========================================
  console.log("2️⃣  Testing create instruction...");
  try {
    const mint = generateSigner(umi);
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);

    const bondingCurvePda = umi.eddsa.findPda(getCoinfunProgramId(umi), [
      string({ size: "variable" }).serialize("bonding_curve"),
      publicKeySerializer().serialize(mint.publicKey),
    ])[0];

    const bondingCurveAta = getAssociatedTokenAddress(
      mint.publicKey,
      bondingCurvePda
    );
    const metadataPda = getMetadataPda(mint.publicKey);

    await create(umi, {
      signer: authority,
      creator: creator.publicKey,
      mint,
      global: globalPda,
      bondingCurve: bondingCurvePda,
      bondingCurveAta,
      metadataAccount: metadataPda,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenName: "Test Token",
      tokenSymbol: "TEST",
      tokenUri: "https://test.com/token.json",
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Create successful!`);
    console.log(`   ✅ Mint: ${mint.publicKey}`);
    console.log(`   ✅ Bonding curve PDA: ${bondingCurvePda}\n`);

    // Store mint for later use
    (global as any).testMint = mint;
    (global as any).testBondingCurvePda = bondingCurvePda;
  } catch (error) {
    console.error(`   ❌ Create failed:`, error);
    throw error;
  }

  //// ========================================
  //// 4. BUY (First Buy)
  //// ========================================
  console.log("3️⃣  Testing buy instruction (FIRST BUY)...");
  try {
    const mint = (global as any).testMint;
    const bondingCurvePda = (global as any).testBondingCurvePda;
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);

    const bondingCurveAta = getAssociatedTokenAddress(
      mint.publicKey,
      bondingCurvePda
    );
    const buyerAta = getAssociatedTokenAddress(mint.publicKey, buyer.publicKey);
    const reserveAta = getAssociatedTokenAddress(
      mint.publicKey,
      globalReservePda
    );

    const solAmount = BigInt(Math.floor(0.5 * Number(LAMPORTS_PER_SOL)));
    const minTokenOutput = BigInt(0);

    umi.use(keypairIdentity(buyer));

    await buy(umi, {
      signer: buyer,
      bondingCurve: bondingCurvePda,
      bondingCurveAta,
      userAta: buyerAta,
      mint: mint.publicKey,
      global: globalPda,
      platformFeeRecipient: feeRecipient.publicKey,
      globalReserve: globalReservePda,
      reserveAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      solAmount,
      minTokenOutput,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ First buy successful!\n`);
  } catch (error) {
    console.error(`   ❌ First buy failed:`, error);
    throw error;
  }

  //// ========================================
  //// 5. BUY (Subsequent Buy)
  //// ========================================
  console.log("4️⃣  Testing buy instruction (SUBSEQUENT BUY)...");
  try {
    const mint = (global as any).testMint;
    const bondingCurvePda = (global as any).testBondingCurvePda;
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);

    const bondingCurveAta = getAssociatedTokenAddress(
      mint.publicKey,
      bondingCurvePda
    );
    const buyerAta = getAssociatedTokenAddress(mint.publicKey, buyer.publicKey);
    const reserveAta = getAssociatedTokenAddress(
      mint.publicKey,
      globalReservePda
    );

    const solAmount = BigInt(Math.floor(0.5 * Number(LAMPORTS_PER_SOL)));
    const minTokenOutput = BigInt(0);

    await buy(umi, {
      signer: buyer,
      bondingCurve: bondingCurvePda,
      bondingCurveAta,
      userAta: buyerAta,
      mint: mint.publicKey,
      global: globalPda,
      platformFeeRecipient: feeRecipient.publicKey,
      globalReserve: globalReservePda,
      reserveAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      solAmount,
      minTokenOutput,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Subsequent buy successful!\n`);
  } catch (error) {
    console.error(`   ❌ Subsequent buy failed:`, error);
    throw error;
  }

  //// ========================================
  //// 6. SELL
  //// ========================================
  console.log("5️⃣  Testing sell instruction...");
  try {
    const mint = (global as any).testMint;
    const bondingCurvePda = (global as any).testBondingCurvePda;
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);

    const bondingCurveAta = getAssociatedTokenAddress(
      mint.publicKey,
      bondingCurvePda
    );
    const buyerAta = getAssociatedTokenAddress(mint.publicKey, buyer.publicKey);
    const reserveAta = getAssociatedTokenAddress(
      mint.publicKey,
      globalReservePda
    );

    // Get token balance first
    const tokenAccount = await umi.rpc.getAccount(buyerAta);
    if (!tokenAccount.exists) {
      throw new Error("Buyer token account not found");
    }

    // For simplicity, we'll sell a reasonable amount
    // In production, you'd decode the token account data properly
    const tokenAmount = BigInt(100_000_000_000); // A reasonable amount to sell
    const minSolOutput = BigInt(0);

    await sell(umi, {
      signer: buyer,
      bondingCurve: bondingCurvePda,
      bondingCurveAta,
      userAta: buyerAta,
      mint: mint.publicKey,
      global: globalPda,
      platformFeeRecipient: feeRecipient.publicKey,
      globalReserve: globalReservePda,
      reserveAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenAmount,
      minSolOutput,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Sell successful!\n`);
  } catch (error) {
    console.error(`   ❌ Sell failed:`, error);
    throw error;
  }

  //// ========================================
  //// 7. UPDATE GLOBAL CONFIG
  //// ========================================
  console.log("6️⃣  Testing updateGlobalConfig instruction...");
  try {
    umi.use(keypairIdentity(authority));
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);

    const newPlatformFeeBps = BigInt(150); // 1.5%
    const newReserveFeeBps = BigInt(350); // 3.5%

    await updateGlobalConfig(umi, {
      authority,
      newAuthority: authority.publicKey,
      newPlatformFeeRecipient: feeRecipient.publicKey,
      newPlatformTradeFeeBps: newPlatformFeeBps,
      newReserveTradeFeeBps: newReserveFeeBps,
      newInitialVirtualTokenReserves: initialVirtualTokenReserves,
      newInitialVirtualSolReserves: initialVirtualSolReserves,
      newTokenTotalSupply: tokenTotalSupply,
      newGraduationThreshold: graduationThreshold,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Update global config successful!\n`);
  } catch (error) {
    console.error(`   ❌ Update global config failed:`, error);
    throw error;
  }

  //// ========================================
  //// 8. WITHDRAW (after graduation)
  //// ========================================
  console.log("7️⃣  Testing withdraw instruction...");
  try {
    // Create a new token for withdrawal test
    const withdrawMint = generateSigner(umi);
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);

    const withdrawBondingCurvePda = umi.eddsa.findPda(
      getCoinfunProgramId(umi),
      [
        string({ size: "variable" }).serialize("bonding_curve"),
        publicKeySerializer().serialize(withdrawMint.publicKey),
      ]
    )[0];

    const withdrawBondingCurveAta = getAssociatedTokenAddress(
      withdrawMint.publicKey,
      withdrawBondingCurvePda
    );

    const withdrawMetadataPda = getMetadataPda(withdrawMint.publicKey);

    // Create the token
    await create(umi, {
      signer: authority,
      creator: creator.publicKey,
      mint: withdrawMint,
      global: globalPda,
      bondingCurve: withdrawBondingCurvePda,
      bondingCurveAta: withdrawBondingCurveAta,
      metadataAccount: withdrawMetadataPda,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenName: "Withdraw Test Token",
      tokenSymbol: "WTH",
      tokenUri: "https://test.com/withdraw.json",
    }).sendAndConfirm(umi, options);

    // Graduate the curve by buying enough SOL
    const graduateBuyer = generateSigner(umi);
    await umi.rpc.airdrop(graduateBuyer.publicKey, sol(100), options.confirm);
    umi.use(keypairIdentity(graduateBuyer));

    const graduateBuyerAta = getAssociatedTokenAddress(
      withdrawMint.publicKey,
      graduateBuyer.publicKey
    );
    const withdrawReserveAta = getAssociatedTokenAddress(
      withdrawMint.publicKey,
      globalReservePda
    );

    const graduateAmount = BigInt(90) * LAMPORTS_PER_SOL;

    await buy(umi, {
      signer: graduateBuyer,
      bondingCurve: withdrawBondingCurvePda,
      bondingCurveAta: withdrawBondingCurveAta,
      userAta: graduateBuyerAta,
      mint: withdrawMint.publicKey,
      global: globalPda,
      platformFeeRecipient: feeRecipient.publicKey,
      globalReserve: globalReservePda,
      reserveAta: withdrawReserveAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      solAmount: graduateAmount,
      minTokenOutput: BigInt(0),
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Curve graduated`);

    // No cooldown needed anymore - withdraw immediately
    umi.use(keypairIdentity(authority));

    const authorityAta = getAssociatedTokenAddress(
      withdrawMint.publicKey,
      authority.publicKey
    );

    // Check if authority ATA exists, create it if not
    const authorityAtaAccount = await umi.rpc.getAccount(authorityAta);
    if (!authorityAtaAccount.exists) {
      console.log("   Creating authority ATA...");
      // Create associated token account instruction
      // The createAssociatedTokenAccount instruction has no data and uses these accounts:
      // 0. Payer (signer, writable)
      // 1. ATA (writable)
      // 2. Owner (readonly)
      // 3. Mint (readonly)
      // 4. System Program (readonly)
      // 5. Token Program (readonly)
      const SYSTEM_PROGRAM_ID = publicKey("11111111111111111111111111111111");

      const createAtaTx = transactionBuilder([
        {
          instruction: {
            programId: ASSOCIATED_TOKEN_PROGRAM_ID,
            keys: [
              { pubkey: authority.publicKey, isSigner: true, isWritable: true },
              { pubkey: authorityAta, isSigner: false, isWritable: true },
              {
                pubkey: authority.publicKey,
                isSigner: false,
                isWritable: false,
              },
              {
                pubkey: withdrawMint.publicKey,
                isSigner: false,
                isWritable: false,
              },
              { pubkey: SYSTEM_PROGRAM_ID, isSigner: false, isWritable: false },
              { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
            ],
            data: new Uint8Array(),
          },
          signers: [authority],
          bytesCreatedOnChain: 0,
        },
      ]);

      await createAtaTx.sendAndConfirm(umi, options);
      console.log("   ✅ Authority ATA created");
    }

    // Build withdraw transaction
    const withdrawTx = withdraw(umi, {
      authority,
      global: globalPda,
      mint: withdrawMint.publicKey,
      bondingCurve: withdrawBondingCurvePda,
      bondingCurveAta: withdrawBondingCurveAta,
      authorityAta,
      tokenProgram: TOKEN_PROGRAM_ID,
    });

    await withdrawTx.sendAndConfirm(umi, options);

    console.log(`   ✅ Withdraw successful!\n`);
  } catch (error) {
    console.error(`   ❌ Withdraw failed:`, error);
    throw error;
  }

  //// ========================================
  //// 9. WITHDRAW RESERVE
  //// ========================================
  console.log("8️⃣  Testing withdrawReserve instruction...");
  try {
    // Use the withdraw mint's reserve ATA
    const withdrawMint = (global as any).testMint; // Can reuse the first test mint
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);
    const bondingCurvePda = (global as any).testBondingCurvePda;

    const reserveAta = getAssociatedTokenAddress(
      withdrawMint.publicKey,
      globalReservePda
    );
    const authorityAta = getAssociatedTokenAddress(
      withdrawMint.publicKey,
      authority.publicKey
    );

    // Check if authority ATA exists for the test mint, create it if not
    const testAuthorityAtaAccount = await umi.rpc.getAccount(authorityAta);
    if (!testAuthorityAtaAccount.exists) {
      console.log("   Creating authority ATA for test mint...");
      const SYSTEM_PROGRAM_ID = publicKey("11111111111111111111111111111111");

      const createAtaTx = transactionBuilder([
        {
          instruction: {
            programId: ASSOCIATED_TOKEN_PROGRAM_ID,
            keys: [
              { pubkey: authority.publicKey, isSigner: true, isWritable: true },
              { pubkey: authorityAta, isSigner: false, isWritable: true },
              {
                pubkey: authority.publicKey,
                isSigner: false,
                isWritable: false,
              },
              {
                pubkey: withdrawMint.publicKey,
                isSigner: false,
                isWritable: false,
              },
              { pubkey: SYSTEM_PROGRAM_ID, isSigner: false, isWritable: false },
              { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
            ],
            data: new Uint8Array(),
          },
          signers: [authority],
          bytesCreatedOnChain: 0,
        },
      ]);

      await createAtaTx.sendAndConfirm(umi, options);
      console.log("   ✅ Authority ATA created");
    }

    // Withdraw half of the reserve tokens
    const amount = BigInt(1_000_000_000); // 1000 tokens (assuming 6 decimals)

    await withdrawReserve(umi, {
      authority,
      global: globalPda,
      globalReserve: globalReservePda,
      mint: withdrawMint.publicKey,
      bondingCurve: bondingCurvePda,
      reserveAta,
      authorityAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      amount,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Withdraw reserve successful!\n`);
  } catch (error) {
    console.error(`   ❌ Withdraw reserve failed:`, error);
    throw error;
  }

  //// ========================================
  //// 10. DEPOSIT TO RESERVE
  //// ========================================
  console.log("9️⃣  Testing depositToReserve instruction...");
  try {
    // This simulates the off-chain process after Meteora DAMMV2 graduation:
    // 1. Liquidity moves to Meteora DAMMV2 pool (5% fee)
    // 2. Fees are collected from Meteora
    // 3. 20% goes to platform, 80% deposited to reserve
    // Here we test depositing the 80% portion to the reserve

    const testMint = (global as any).testMint;
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalReservePda = findPda([
      string({ size: "variable" }).serialize("reserve"),
    ]);
    const bondingCurvePda = (global as any).testBondingCurvePda;

    const reserveAta = getAssociatedTokenAddress(
      testMint.publicKey,
      globalReservePda
    );
    const authorityAta = getAssociatedTokenAddress(
      testMint.publicKey,
      authority.publicKey
    );

    // First, withdraw some tokens from reserve to authority
    // This simulates collecting Meteora fees off-chain
    console.log("   📤 First withdrawing tokens from reserve (simulating Meteora fee collection)...");
    const withdrawAmount = BigInt(2_000_000_000); // 2000 tokens
    
    await withdrawReserve(umi, {
      authority,
      global: globalPda,
      globalReserve: globalReservePda,
      mint: testMint.publicKey,
      bondingCurve: bondingCurvePda,
      reserveAta,
      authorityAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      amount: withdrawAmount,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Withdrew ${withdrawAmount} tokens (simulating Meteora fees collected)`);

    // Now authority has tokens (simulating 80% of Meteora fees to deposit)
    // Deposit portion back to reserve (80% of collected fees)
    const depositAmount = BigInt(1_000_000_000); // 1000 tokens (50% of withdrawn, simulating 80% of fees)

    console.log("   📥 Now depositing tokens back to reserve (80% portion)...");
    await depositToReserve(umi, {
      authority,
      global: globalPda,
      globalReserve: globalReservePda,
      mint: testMint.publicKey,
      bondingCurve: bondingCurvePda,
      reserveAta,
      authorityAta,
      tokenProgram: TOKEN_PROGRAM_ID,
      amount: depositAmount,
    }).sendAndConfirm(umi, options);

    console.log(`   ✅ Deposit to reserve successful!`);
    console.log(`   📊 Deposited ${depositAmount} tokens from Meteora fees to reserve`);
    console.log(`   ℹ️  Real scenario: 20% to platform, 80% to reserve\n`);
  } catch (error) {
    console.error(`   ❌ Deposit to reserve failed:`, error);
    throw error;
  }

  console.log("🎉 All tests completed successfully!");
}

main()
  .then(() => {
    console.log("🚀 - Done!");
  })
  .catch((error) => {
    console.error("❌ - Error:", error);
    process.exit(1);
  });
