/**
 * Utility script to extract and log the base58-encoded secret key from a Solana keypair file
 *
 * Usage:
 *   ts-node clients/get-secret-key.ts [keypair-path]
 *
 * If no path is provided, defaults to ~/.config/solana/id.json
 */

import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { Keypair } from "@solana/web3.js";
import bs58 from "bs58";

async function main() {
  // Get keypair path from command line argument or use default
  const keypairPathArg = process.argv[2];
  const keypairPath = keypairPathArg
    ? path.resolve(keypairPathArg)
    : path.join(os.homedir(), ".config", "solana", "id.json");

  console.log(`📁 Reading keypair from: ${keypairPath}\n`);

  try {
    // Check if file exists
    if (!fs.existsSync(keypairPath)) {
      console.error(`❌ Error: Keypair file not found at ${keypairPath}`);
      process.exit(1);
    }

    // Read and parse keypair file
    const keypairFile = fs.readFileSync(keypairPath, "utf-8");
    const keypairArray = JSON.parse(keypairFile);

    // Create keypair from secret key
    const keypair = Keypair.fromSecretKey(new Uint8Array(keypairArray));

    // Encode secret key to base58
    const base58SecretKey = bs58.encode(keypair.secretKey);

    // Log results
    console.log("✅ Keypair loaded successfully!\n");
    console.log("📋 Keypair Information:");
    console.log(`   Public Key: ${keypair.publicKey.toBase58()}`);
    console.log(`   Secret Key (Base58): ${base58SecretKey}\n`);

    // Warning about security
    console.log("⚠️  WARNING: Keep your secret key secure! Never share it publicly.");
  } catch (error) {
    console.error(`❌ Error reading keypair:`, error);
    process.exit(1);
  }
}

main()
  .then(() => {
    console.log("🚀 - Done!");
  })
  .catch((error) => {
    console.error("❌ - Error:", error);
    process.exit(1);
  });

