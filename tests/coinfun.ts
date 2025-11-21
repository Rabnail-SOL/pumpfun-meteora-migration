import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Coinfun } from "../target/types/coinfun";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import { expect } from "chai";
import {
  getAssociatedTokenAddressSync,
  getAccount,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";

async function getTokenBalance(
  provider: anchor.Provider,
  ata: PublicKey
): Promise<number> {
  try {
    const account = await getAccount(provider.connection, ata);
    return Number(account.amount);
  } catch (e) {
    return 0;
  }
}

async function getSolBalance(
  provider: anchor.Provider,
  pubkey: PublicKey
): Promise<number> {
  return await provider.connection.getBalance(pubkey);
}

describe("coinfun", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.coinfun as Program<Coinfun>;
  const authority = (provider.wallet as anchor.Wallet).payer;

  // Define all keypairs and constants
  const platformFeeRecipient = Keypair.generate();
  const creator = Keypair.generate();
  const buyer = Keypair.generate();
  const secondBuyer = Keypair.generate();

  const initialVirtualTokenReserves = new anchor.BN(1_073_000_191 * 1e6);
  const initialVirtualSolReserves = new anchor.BN(30 * LAMPORTS_PER_SOL);
  const tokenTotalSupply = new anchor.BN(1_000_000_000 * 1e6);

  const graduationThreshold = new anchor.BN(2 * LAMPORTS_PER_SOL); // Lower for testing
  const platformTradeFeeBps = new anchor.BN(100); // 1% to platform
  const reserveTradeFeeBps = new anchor.BN(400); // 4% to reserve (total 5%)

  // PDAs and Keypairs
  let global: PublicKey;
  let globalReserve: PublicKey;
  const mint = Keypair.generate();
  let bondingCurve: PublicKey;
  let bondingCurveAta: PublicKey;
  let reserveAta: PublicKey;

  before(async () => {
    // Fund all wallets
    const fundTx = new anchor.web3.Transaction()
      .add(
        SystemProgram.transfer({
          fromPubkey: authority.publicKey,
          toPubkey: buyer.publicKey,
          lamports: 10 * LAMPORTS_PER_SOL,
        })
      )
      .add(
        SystemProgram.transfer({
          fromPubkey: authority.publicKey,
          toPubkey: secondBuyer.publicKey,
          lamports: 10 * LAMPORTS_PER_SOL,
        })
      )
      .add(
        SystemProgram.transfer({
          fromPubkey: authority.publicKey,
          toPubkey: creator.publicKey,
          lamports: 2 * LAMPORTS_PER_SOL,
        })
      );
    await provider.sendAndConfirm(fundTx);

    // Derive all PDAs
    [global] = PublicKey.findProgramAddressSync(
      [Buffer.from("global")],
      program.programId
    );
    [globalReserve] = PublicKey.findProgramAddressSync(
      [Buffer.from("reserve")],
      program.programId
    );
    [bondingCurve] = PublicKey.findProgramAddressSync(
      [Buffer.from("bonding_curve"), mint.publicKey.toBuffer()],
      program.programId
    );
    bondingCurveAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      bondingCurve,
      true
    );
    reserveAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      globalReserve,
      true
    );
  });

  it("Is initialized and checks global state", async () => {
    await program.methods
      .initialize(
        initialVirtualTokenReserves,
        initialVirtualSolReserves,
        tokenTotalSupply,
        platformTradeFeeBps,
        reserveTradeFeeBps,
        platformFeeRecipient.publicKey,
        graduationThreshold
      )
      .rpc();

    const globalData = await program.account.global.fetch(global);
    expect(globalData.authority.toBase58()).to.eq(
      authority.publicKey.toBase58()
    );
    expect(globalData.platformFeeRecipient.toBase58()).to.eq(
      platformFeeRecipient.publicKey.toBase58()
    );
    expect(globalData.reserve.toBase58()).to.eq(globalReserve.toBase58());
    expect(globalData.platformTradeFeeBps.toString()).to.eq(
      platformTradeFeeBps.toString()
    );
    expect(globalData.reserveTradeFeeBps.toString()).to.eq(
      reserveTradeFeeBps.toString()
    );
    expect(globalData.tokenTotalSupply.toString()).to.eq(
      tokenTotalSupply.toString()
    );
    expect(globalData.graduationThreshold.toString()).to.eq(
      graduationThreshold.toString()
    );
  });

  it("Creates a new token and checks state and TokenCreated event", async () => {
    const beforeSol = await getSolBalance(provider, authority.publicKey);
    
    // Listen for tokenCreated event
    let eventReceived = false;
    const listener = program.addEventListener("tokenCreated", (event, slot) => {
      expect(event.mint.toBase58()).to.eq(mint.publicKey.toBase58());
      expect(event.creator.toBase58()).to.eq(creator.publicKey.toBase58());
      eventReceived = true;
    });

    await program.methods
      .create("Test Token", "TEST", "https://test.com/token.json")
      .accounts({
        signer: authority.publicKey,
        creator: creator.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        bondingCurveAta: bondingCurveAta,
      })
      .signers([mint])
      .rpc();
    
    // Wait a bit for event
    await new Promise((resolve) => setTimeout(resolve, 1000));
    await program.removeEventListener(listener);
    expect(eventReceived).to.be.true;

    const afterSol = await getSolBalance(provider, authority.publicKey);
    expect(afterSol).to.be.lt(beforeSol); // Paid rent for new accounts

    const bondingCurveData = await program.account.bondingCurve.fetch(
      bondingCurve
    );
    expect(bondingCurveData.mint.toBase58()).to.eq(mint.publicKey.toBase58());
    expect(bondingCurveData.creator.toBase58()).to.eq(
      creator.publicKey.toBase58()
    );
    expect(bondingCurveData.realSolReserves.toNumber()).to.eq(0);
    expect(bondingCurveData.realTokenReserves.toString()).to.eq(
      tokenTotalSupply.toString()
    );
    expect(bondingCurveData.complete).to.be.false;

    // Check token balances
    const bondingCurveAtaBalance = await getTokenBalance(
      provider,
      bondingCurveAta
    );
    expect(bondingCurveAtaBalance).to.eq(tokenTotalSupply.toNumber());
  });

  it("Allows buy with consistent fee distribution and reserve token purchase", async () => {
    const solAmountToBuy = new anchor.BN(1 * LAMPORTS_PER_SOL);
    const buyerAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      buyer.publicKey
    );
    
    const beforeBuyerSol = await getSolBalance(provider, buyer.publicKey);
    const beforePlatformSol = await getSolBalance(provider, platformFeeRecipient.publicKey);
    const beforeCurveSol = await getSolBalance(provider, bondingCurve);
    const beforeCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const beforeReserveTokens = await getTokenBalance(provider, reserveAta);

    // Listen for trade event
    let tradeEventReceived = false;
    const listener = program.addEventListener("trade", (event, slot) => {
      expect(event.mint.toBase58()).to.eq(mint.publicKey.toBase58());
      expect(event.trader.toBase58()).to.eq(buyer.publicKey.toBase58());
      expect(JSON.stringify(event.side)).to.include("buy");
      expect(event.solAmount.toString()).to.eq(solAmountToBuy.toString());
      expect(event.tokenAmount.toNumber()).to.be.gt(0);
      tradeEventReceived = true;
    });

    await program.methods
      .buy(solAmountToBuy, new anchor.BN(0))
      .accounts({
        signer: buyer.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: reserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([buyer])
      .rpc();

    await new Promise((resolve) => setTimeout(resolve, 1000));
    await program.removeEventListener(listener);
    expect(tradeEventReceived).to.be.true;

    const afterBuyerSol = await getSolBalance(provider, buyer.publicKey);
    const afterPlatformSol = await getSolBalance(provider, platformFeeRecipient.publicKey);
    const afterCurveSol = await getSolBalance(provider, bondingCurve);
    const afterCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const afterReserveTokens = await getTokenBalance(provider, reserveAta);

    // Calculate expected fees
    const expectedPlatformFee = solAmountToBuy
      .mul(platformTradeFeeBps)
      .div(new anchor.BN(10000));
    const expectedReserveFee = solAmountToBuy
      .mul(reserveTradeFeeBps)
      .div(new anchor.BN(10000));
    const expectedSolToCurve = solAmountToBuy
      .sub(expectedPlatformFee)
      .sub(expectedReserveFee)
      .add(expectedReserveFee); // Reserve fee goes to curve

    // Verify platform fee
    expect(afterPlatformSol - beforePlatformSol).to.eq(expectedPlatformFee.toNumber());
    
    // Verify curve receives user's portion + reserve fee
    expect(afterCurveSol - beforeCurveSol).to.eq(expectedSolToCurve.toNumber());
    
    // Verify buyer paid full amount
    expect(beforeBuyerSol - afterBuyerSol).to.be.gte(solAmountToBuy.toNumber());
    
    // Verify reserve received tokens (reserve fee was used to buy tokens)
    expect(afterReserveTokens).to.be.gt(beforeReserveTokens);
    
    // Verify curve state updated correctly
    expect(afterCurveData.realSolReserves.toNumber()).to.be.gt(beforeCurveData.realSolReserves.toNumber());
  });

  it("Allows subsequent buy and verifies consistent fee model", async () => {
    const solAmountToBuy = new anchor.BN(1 * LAMPORTS_PER_SOL);
    const secondBuyerAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      secondBuyer.publicKey
    );

    const beforeSecondBuyerSol = await getSolBalance(provider, secondBuyer.publicKey);
    const beforePlatformSol = await getSolBalance(provider, platformFeeRecipient.publicKey);
    const beforeCurveSol = await getSolBalance(provider, bondingCurve);
    const beforeCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const beforeReserveTokens = await getTokenBalance(provider, reserveAta);

    await program.methods
      .buy(solAmountToBuy, new anchor.BN(0))
      .accounts({
        signer: secondBuyer.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: reserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([secondBuyer])
      .rpc();

    const afterSecondBuyerSol = await getSolBalance(provider, secondBuyer.publicKey);
    const afterPlatformSol = await getSolBalance(provider, platformFeeRecipient.publicKey);
    const afterCurveSol = await getSolBalance(provider, bondingCurve);
    const afterCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const afterReserveTokens = await getTokenBalance(provider, reserveAta);

    // Calculate expected fees (same as first buy - consistent model)
    const expectedPlatformFee = solAmountToBuy
      .mul(platformTradeFeeBps)
      .div(new anchor.BN(10000));
    const expectedReserveFee = solAmountToBuy
      .mul(reserveTradeFeeBps)
      .div(new anchor.BN(10000));

    // Verify fees are consistent
    expect(afterPlatformSol - beforePlatformSol).to.eq(expectedPlatformFee.toNumber());
    expect(afterReserveTokens).to.be.gt(beforeReserveTokens); // Reserve got more tokens
    expect(afterCurveSol - beforeCurveSol).to.be.gt(0);
  });

  it("Allows sell with reserve token purchase and comprehensive validation", async () => {
    const buyerAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      buyer.publicKey
    );
    const tokenBalance = await getTokenBalance(provider, buyerAta);
    const tokenAmountToSell = new anchor.BN(Math.floor(tokenBalance / 2));

    const beforeBuyerSol = await getSolBalance(provider, buyer.publicKey);
    const beforePlatformSol = await getSolBalance(provider, platformFeeRecipient.publicKey);
    const beforeCurveSol = await getSolBalance(provider, bondingCurve);
    const beforeCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const beforeReserveTokens = await getTokenBalance(provider, reserveAta);

    // Listen for trade event
    let sellEventReceived = false;
    const listener = program.addEventListener("trade", (event, slot) => {
      if (JSON.stringify(event.side).includes("sell")) {
        expect(event.mint.toBase58()).to.eq(mint.publicKey.toBase58());
        expect(event.trader.toBase58()).to.eq(buyer.publicKey.toBase58());
        expect(event.tokenAmount.toString()).to.eq(tokenAmountToSell.toString());
        sellEventReceived = true;
      }
    });

    await program.methods
      .sell(tokenAmountToSell, new anchor.BN(0))
      .accounts({
        signer: buyer.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: reserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([buyer])
      .rpc();

    await new Promise((resolve) => setTimeout(resolve, 1000));
    await program.removeEventListener(listener);
    expect(sellEventReceived).to.be.true;

    const afterBuyerSol = await getSolBalance(provider, buyer.publicKey);
    const afterPlatformSol = await getSolBalance(provider, platformFeeRecipient.publicKey);
    const afterCurveSol = await getSolBalance(provider, bondingCurve);
    const afterCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const afterReserveTokens = await getTokenBalance(provider, reserveAta);

    // Verify buyer received SOL (net of fees)
    const solReceived = afterBuyerSol - beforeBuyerSol;
    expect(solReceived).to.be.gt(0);

    // Verify platform fee
    const platformFeePaid = afterPlatformSol - beforePlatformSol;
    expect(platformFeePaid).to.be.gt(0);

    // Verify reserve got tokens (bought with reserve fee)
    expect(afterReserveTokens).to.be.gt(beforeReserveTokens);

    // Verify SOL accounting: curve lost (solReceived + platformFee + reserveFee)
    // but reserve fee stayed in curve, so curve lost (solReceived + platformFee)
    const solRemovedFromCurve = beforeCurveSol - afterCurveSol;
    expect(solRemovedFromCurve).to.be.approximately(solReceived + platformFeePaid, 100000);

    // Verify curve state
    expect(afterCurveData.realSolReserves.toNumber()).to.be.lt(beforeCurveData.realSolReserves.toNumber());
  });

  it("Tests 30% fee cap validation", async () => {
    // Try to update with fees > 30%
    try {
      await program.methods
        .updateGlobalConfig(
          authority.publicKey,
          platformFeeRecipient.publicKey,
          new anchor.BN(2000), // 20%
          new anchor.BN(1100), // 11% (total 31% > 30%)
          initialVirtualTokenReserves,
          initialVirtualSolReserves,
          tokenTotalSupply,
          graduationThreshold
        )
        .accounts({ authority: authority.publicKey })
        .rpc();
      expect.fail("Should have failed with FeeTooHigh");
    } catch (e) {
      expect(
        e.toString().includes("FeeTooHigh") ||
        e.toString().includes("custom program error")
      ).to.be.true;
    }

    // Try exactly 30% - should succeed
    await program.methods
      .updateGlobalConfig(
        authority.publicKey,
        platformFeeRecipient.publicKey,
        new anchor.BN(1500), // 15%
        new anchor.BN(1500), // 15% (total 30%)
        initialVirtualTokenReserves,
        initialVirtualSolReserves,
        tokenTotalSupply,
        graduationThreshold
      )
      .accounts({ authority: authority.publicKey })
      .rpc();

    const globalData = await program.account.global.fetch(global);
    expect(globalData.platformTradeFeeBps.toNumber() + globalData.reserveTradeFeeBps.toNumber()).to.eq(3000);

    // Reset to original fees
    await program.methods
      .updateGlobalConfig(
        authority.publicKey,
        platformFeeRecipient.publicKey,
        platformTradeFeeBps,
        reserveTradeFeeBps,
        initialVirtualTokenReserves,
        initialVirtualSolReserves,
        tokenTotalSupply,
        graduationThreshold
      )
      .accounts({ authority: authority.publicKey })
      .rpc();
  });

  it("Graduates curve and emits CurveComplete event", async () => {
    const beforeCurveData = await program.account.bondingCurve.fetch(bondingCurve);
    const solNeeded = graduationThreshold
      .sub(beforeCurveData.realSolReserves)
      .add(new anchor.BN(0.5 * LAMPORTS_PER_SOL)); // Add extra to ensure graduation

    // Listen for curveComplete event
    let curveCompleteReceived = false;
    const listener = program.addEventListener("curveComplete", (event, slot) => {
      expect(event.mint.toBase58()).to.eq(mint.publicKey.toBase58());
      expect(event.bondingCurve.toBase58()).to.eq(bondingCurve.toBase58());
      curveCompleteReceived = true;
    });

    await program.methods
      .buy(solNeeded, new anchor.BN(0))
      .accounts({
        signer: buyer.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: reserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([buyer])
      .rpc();

    await new Promise((resolve) => setTimeout(resolve, 1000));
    await program.removeEventListener(listener);
    expect(curveCompleteReceived).to.be.true;

    const curveData = await program.account.bondingCurve.fetch(bondingCurve);
    expect(curveData.complete).to.be.true;
    expect(curveData.realSolReserves.toNumber()).to.be.gte(
      graduationThreshold.toNumber()
    );

    // Try to buy again, should fail
    try {
      await program.methods
        .buy(new anchor.BN(0.1 * LAMPORTS_PER_SOL), new anchor.BN(0))
        .accounts({
          signer: buyer.publicKey,
          mint: mint.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .accountsPartial({
          reserveAta: reserveAta,
          platformFeeRecipient: platformFeeRecipient.publicKey,
        })
        .signers([buyer])
        .rpc();
      expect.fail("Should have failed to buy on a complete curve");
    } catch (e) {
      expect(e.toString()).to.include("BondingCurveComplete");
    }
  });

  it("Allows authority to withdraw from graduated curve (no cooldown)", async () => {
    const authorityAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      authority.publicKey
    );
    const beforeAuthorityToken = await getTokenBalance(provider, authorityAta);
    const beforeAuthoritySol = await getSolBalance(provider, authority.publicKey);
    const beforeCurveToken = await getTokenBalance(provider, bondingCurveAta);

    // Create the ATA if it doesn't exist
    try {
      const createAtaIx = createAssociatedTokenAccountInstruction(
        authority.publicKey,
        authorityAta,
        authority.publicKey,
        mint.publicKey
      );
      const setupTx = new anchor.web3.Transaction().add(createAtaIx);
      await provider.sendAndConfirm(setupTx, [], { commitment: "confirmed" });
    } catch (e) {
      // ATA might already exist
    }

    await program.methods
      .withdraw()
      .accounts({
        authority: authority.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    const afterAuthorityToken = await getTokenBalance(provider, authorityAta);
    const afterAuthoritySol = await getSolBalance(provider, authority.publicKey);
    const finalCurveTokenBalance = await getTokenBalance(provider, bondingCurveAta);

    // Verify all tokens withdrawn
    expect(finalCurveTokenBalance).to.eq(0);
    expect(afterAuthorityToken).to.eq(beforeAuthorityToken + beforeCurveToken);
    
    // Verify SOL withdrawn (minus rent)
    expect(afterAuthoritySol).to.be.gt(beforeAuthoritySol);
  });

  it("Allows authority to withdraw specific amount from reserve (no cooldown)", async () => {
    const authorityAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      authority.publicKey
    );
    const reserveBalance = await getTokenBalance(provider, reserveAta);
    expect(reserveBalance).to.be.gt(0); // Reserve should have tokens from all the trades

    const withdrawAmount = new anchor.BN(Math.floor(reserveBalance / 2));
    const beforeAuthorityToken = await getTokenBalance(provider, authorityAta);
    const beforeReserveToken = await getTokenBalance(provider, reserveAta);

    await program.methods
      .withdrawReserve(withdrawAmount)
      .accounts({
        authority: authority.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    const afterAuthorityToken = await getTokenBalance(provider, authorityAta);
    const afterReserveToken = await getTokenBalance(provider, reserveAta);

    // Verify specific amount withdrawn
    expect(afterAuthorityToken).to.eq(beforeAuthorityToken + withdrawAmount.toNumber());
    expect(afterReserveToken).to.eq(beforeReserveToken - withdrawAmount.toNumber());

    // Withdraw again immediately (no cooldown)
    const secondWithdrawAmount = new anchor.BN(Math.floor(afterReserveToken / 2));
    await program.methods
      .withdrawReserve(secondWithdrawAmount)
      .accounts({
        authority: authority.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    const finalReserveToken = await getTokenBalance(provider, reserveAta);
    expect(finalReserveToken).to.eq(afterReserveToken - secondWithdrawAmount.toNumber());
  });

  it("Allows authority to deposit tokens to reserve (simulating Meteora fees)", async () => {
    // This simulates the off-chain process where:
    // 1. Liquidity is moved to Meteora DAMMV2 pool after graduation
    // 2. Fees are collected from Meteora (5% fee in pool)
    // 3. 20% of collected fees go to platform, 80% to reserve
    // Here we're testing the deposit to reserve part (the 80%)

    const authorityAta = getAssociatedTokenAddressSync(
      mint.publicKey,
      authority.publicKey
    );

    // First, withdraw some tokens from reserve to authority (simulating collected Meteora fees)
    const reserveBalanceBeforeWithdraw = await getTokenBalance(provider, reserveAta);
    const withdrawForDeposit = new anchor.BN(Math.floor(reserveBalanceBeforeWithdraw / 3));
    
    await program.methods
      .withdrawReserve(withdrawForDeposit)
      .accounts({
        authority: authority.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    // Now authority has tokens (simulating 80% of Meteora fees collected)
    const beforeAuthorityToken = await getTokenBalance(provider, authorityAta);
    const beforeReserveToken = await getTokenBalance(provider, reserveAta);
    
    // Deposit tokens back to reserve (80% of collected fees)
    const depositAmount = new anchor.BN(1_000_000_000); // 1000 tokens (6 decimals)
    
    // Make sure authority has enough tokens
    expect(beforeAuthorityToken).to.be.gte(depositAmount.toNumber());

    await program.methods
      .depositToReserve(depositAmount)
      .accounts({
        authority: authority.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    const afterAuthorityToken = await getTokenBalance(provider, authorityAta);
    const afterReserveToken = await getTokenBalance(provider, reserveAta);

    // Verify tokens transferred from authority to reserve
    expect(afterAuthorityToken).to.eq(beforeAuthorityToken - depositAmount.toNumber());
    expect(afterReserveToken).to.eq(beforeReserveToken + depositAmount.toNumber());

    // Test depositing another amount immediately (no cooldown)
    const secondDepositAmount = new anchor.BN(500_000_000); // 500 tokens
    const midAuthorityToken = afterAuthorityToken;
    const midReserveToken = afterReserveToken;

    await program.methods
      .depositToReserve(secondDepositAmount)
      .accounts({
        authority: authority.publicKey,
        mint: mint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    const finalAuthorityToken = await getTokenBalance(provider, authorityAta);
    const finalReserveToken = await getTokenBalance(provider, reserveAta);

    expect(finalAuthorityToken).to.eq(midAuthorityToken - secondDepositAmount.toNumber());
    expect(finalReserveToken).to.eq(midReserveToken + secondDepositAmount.toNumber());
  });

  it("Tests error conditions", async () => {
    // Test withdrawal before graduation
    const newMint = Keypair.generate();
    const [newBondingCurve] = PublicKey.findProgramAddressSync(
      [Buffer.from("bonding_curve"), newMint.publicKey.toBuffer()],
      program.programId
    );
    const newBondingCurveAta = getAssociatedTokenAddressSync(
      newMint.publicKey,
      newBondingCurve,
      true
    );
    await program.methods
      .create("Test Token 2", "TEST2", "https://test.com/token2.json")
      .accounts({
        signer: authority.publicKey,
        creator: creator.publicKey,
        mint: newMint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        bondingCurveAta: newBondingCurveAta,
      })
      .signers([newMint])
      .rpc();

    const newAuthorityAta = getAssociatedTokenAddressSync(
      newMint.publicKey,
      authority.publicKey
    );

    try {
      const createNewAtaIx = createAssociatedTokenAccountInstruction(
        authority.publicKey,
        newAuthorityAta,
        authority.publicKey,
        newMint.publicKey
      );
      const setupNewTx = new anchor.web3.Transaction().add(createNewAtaIx);
      await provider.sendAndConfirm(setupNewTx, [], { commitment: "confirmed" });
    } catch (e) {}

    try {
      await program.methods
        .withdraw()
        .accounts({
          authority: authority.publicKey,
          mint: newMint.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([authority])
        .rpc();
      expect.fail("Should have failed to withdraw before graduation");
    } catch (e) {
      expect(e.toString()).to.include("BondingCurveNotComplete");
    }
  });

  it("Comprehensively tests reserve accumulation across multiple trades", async () => {
    // Create a fresh token
    const testMint = Keypair.generate();
    const [testBondingCurve] = PublicKey.findProgramAddressSync(
      [Buffer.from("bonding_curve"), testMint.publicKey.toBuffer()],
      program.programId
    );
    const testBondingCurveAta = getAssociatedTokenAddressSync(
      testMint.publicKey,
      testBondingCurve,
      true
    );
    const testReserveAta = getAssociatedTokenAddressSync(
      testMint.publicKey,
      globalReserve,
      true
    );

    await program.methods
      .create("Reserve Test Token", "RTT", "https://test.com/rtt.json")
      .accounts({
        signer: authority.publicKey,
        creator: creator.publicKey,
        mint: testMint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        bondingCurveAta: testBondingCurveAta,
      })
      .signers([testMint])
      .rpc();

    const initialReserveBalance = await getTokenBalance(provider, testReserveAta);
    expect(initialReserveBalance).to.eq(0);

    // First buy
    await program.methods
      .buy(new anchor.BN(0.5 * LAMPORTS_PER_SOL), new anchor.BN(0))
      .accounts({
        signer: buyer.publicKey,
        mint: testMint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: testReserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([buyer])
      .rpc();

    const afterFirstBuyReserve = await getTokenBalance(provider, testReserveAta);
    expect(afterFirstBuyReserve).to.be.gt(0);

    // Second buy
    await program.methods
      .buy(new anchor.BN(0.5 * LAMPORTS_PER_SOL), new anchor.BN(0))
      .accounts({
        signer: secondBuyer.publicKey,
        mint: testMint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: testReserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([secondBuyer])
      .rpc();

    const afterSecondBuyReserve = await getTokenBalance(provider, testReserveAta);
    expect(afterSecondBuyReserve).to.be.gt(afterFirstBuyReserve);

    // Sell
    const buyerAta = getAssociatedTokenAddressSync(
      testMint.publicKey,
      buyer.publicKey
    );
    const tokenBalance = await getTokenBalance(provider, buyerAta);
    const sellAmount = new anchor.BN(Math.floor(tokenBalance / 4));

    await program.methods
      .sell(sellAmount, new anchor.BN(0))
      .accounts({
        signer: buyer.publicKey,
        mint: testMint.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .accountsPartial({
        reserveAta: testReserveAta,
        platformFeeRecipient: platformFeeRecipient.publicKey,
      })
      .signers([buyer])
      .rpc();

    const afterSellReserve = await getTokenBalance(provider, testReserveAta);
    expect(afterSellReserve).to.be.gt(afterSecondBuyReserve);

    // Verify reserve accumulated tokens from all trades
    expect(afterSellReserve).to.be.gt(initialReserveBalance);
  });
});
