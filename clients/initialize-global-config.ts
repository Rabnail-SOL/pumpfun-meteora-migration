/**
 * Example script for initializing the global config using the Codama-generated UMI client
 *
 * This script:
 * 1. Initializes the global configuration for the CoinFun program
 */

import { createUmi } from "@metaplex-foundation/umi-bundle-defaults";
import {
  TransactionBuilderSendAndConfirmOptions,
  keypairIdentity,
  sol,
  publicKey,
  PublicKey,
  createSignerFromKeypair,
  Keypair,
} from "@metaplex-foundation/umi";
import { string } from "@metaplex-foundation/umi/serializers";
import { getCoinfunProgramId } from "./generated/umi/src/programs/coinfun";
import { createCoinfunProgram } from "./generated/umi/src/programs/coinfun";
import { initialize } from "./generated/umi/src/instructions";
import { fetchGlobal } from "./generated/umi/src/accounts/global";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { Keypair as SolanaKeypair } from "@solana/web3.js";
import "dotenv/config";

//const umi = createUmi("http://127.0.0.1:8899", { commitment: "processed" });
const umi = createUmi("https://api.devnet.solana.com", {
  commitment: "processed",
});

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
const graduationThreshold = BigInt(2) * LAMPORTS_PER_SOL;

// Helper function to derive PDA
function findPda(seeds: Uint8Array[]): PublicKey {
  const [pda] = umi.eddsa.findPda(getCoinfunProgramId(umi), seeds);
  return pda;
}

async function main() {
  console.log("🚀 Starting CoinFun global config initialization...\n");

  // ========================================
  // 1. AIRDROP FUNDS
  // ========================================
  if (umi.rpc.getEndpoint() == "http://127.0.0.1:8899") {
    console.log("📦 Airdropping funds to authority...");
    try {
      await umi.rpc.airdrop(authority.publicKey, sol(20), options.confirm);
      console.log(
        `   ✅ Airdropped 20 SOL to authority: ${authority.publicKey}`
      );
      console.log(
        `   ✅ Fee recipient is authority: ${feeRecipient.publicKey}\n`
      );
    } catch (error) {
      console.error("   ❌ Error airdropping funds:", error);
      throw error;
    }
  }

  // ========================================
  // 2. INITIALIZE
  // ========================================
  console.log("1️⃣  Initializing global config...");
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
  // 3. FETCH AND LOG GLOBAL ACCOUNT
  // ========================================
  console.log("2️⃣  Fetching global account...");
  try {
    const globalPda = findPda([
      string({ size: "variable" }).serialize("global"),
    ]);
    const globalAccount = await fetchGlobal(umi, globalPda);

    console.log(`   📋 Global Account Data:`);
    console.log(`      Public Key: ${globalAccount.publicKey}`);
    console.log(`      Authority: ${globalAccount.authority}`);
    console.log(
      `      Platform Fee Recipient: ${globalAccount.platformFeeRecipient}`
    );
    console.log(`      Reserve: ${globalAccount.reserve}`);
    console.log(
      `      Initial Virtual Token Reserves: ${globalAccount.initialVirtualTokenReserves.toString()}`
    );
    console.log(
      `      Initial Virtual SOL Reserves: ${globalAccount.initialVirtualSolReserves.toString()} lamports (${
        Number(globalAccount.initialVirtualSolReserves) /
        Number(LAMPORTS_PER_SOL)
      } SOL)`
    );
    console.log(
      `      Token Total Supply: ${globalAccount.tokenTotalSupply.toString()}`
    );
    console.log(
      `      Platform Trade Fee BPS: ${globalAccount.platformTradeFeeBps.toString()} (${
        Number(globalAccount.platformTradeFeeBps) / 100
      }%)`
    );
    console.log(
      `      Reserve Trade Fee BPS: ${globalAccount.reserveTradeFeeBps.toString()} (${
        Number(globalAccount.reserveTradeFeeBps) / 100
      }%)`
    );
    console.log(
      `      Graduation Threshold: ${globalAccount.graduationThreshold.toString()} lamports (${
        Number(globalAccount.graduationThreshold) / Number(LAMPORTS_PER_SOL)
      } SOL)`
    );
    console.log(`   ✅ Global account fetched successfully!\n`);
  } catch (error) {
    console.error(`   ❌ Failed to fetch global account:`, error);
    throw error;
  }

  console.log("🎉 Global config initialization completed successfully!");
}

main()
  .then(() => {
    console.log("🚀 - Done!");
  })
  .catch((error) => {
    console.error("❌ - Error:", error);
    process.exit(1);
  });
