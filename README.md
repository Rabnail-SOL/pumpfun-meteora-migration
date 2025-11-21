# Token Launchpad on Solana

A Solana Anchor program implementing a Pumpfun-style token launchpad with automated market making via bonding curves. Tokens start with a bonding curve and graduate to a standard DEX once they reach a certain SOL threshold.

## Overview

Each token uses a constant product bonding curve that automatically sets the price based on supply and demand. When a token accumulates enough SOL (the "graduation threshold"), it graduates and becomes tradeable on standard DEXs.

### Key Features

- **Bonding Curve AMM**: Automatic price discovery using `virtual_reserves * token_reserves = constant`
- **Fee Distribution**: Trading fees split between platform and reserve, with reserve fees automatically buying tokens
- **Global Reserve System**: Single global reserve PDA with multiple token ATAs for efficient token accumulation
- **Graduation**: Tokens automatically graduate when SOL reserves reach the threshold
- **Event Emission**: On-chain events for off-chain tracking (`TokenCreated`, `Trade`, `CurveComplete`)
- **Fee Cap**: Maximum 30% total fees (platform + reserve) enforced at configuration level

## Smart Contract Instructions

### 1. `initialize`

Sets up the global configuration for the entire program. Must be called once before any tokens can be created.

**Parameters:**
- `initial_virtual_token_reserves`: Starting virtual token reserves for new curves
- `initial_virtual_sol_reserves`: Starting virtual SOL reserves for new curves
- `token_total_supply`: Total supply for tokens created (minted to bonding curve)
- `platform_trade_fee_bps`: Platform's share of trading fees in basis points (100 bps = 1%)
- `reserve_trade_fee_bps`: Reserve's share of trading fees in basis points
- `platform_fee_recipient`: Address that receives platform fees
- `graduation_threshold`: SOL amount needed for a curve to graduate

**Logic:**
- Creates a global PDA account (seeded with `["global"]`) storing all configuration
- Creates a global reserve PDA (seeded with `["reserve"]`) to act as authority for all reserve token accounts
- Validates that `platform_trade_fee_bps + reserve_trade_fee_bps <= 3000` (max 30%)
- Stores the reserve PDA address in the global account for reference

### 2. `create`

Creates a new token with its bonding curve. Each token gets its own bonding curve account.

**Parameters:**
- `token_name`: Token name (e.g., "My Token")
- `token_symbol`: Token symbol (e.g., "MTK")
- `token_uri`: URI pointing to token metadata JSON

**Logic:**
1. Creates a new SPL token mint (6 decimals)
2. Creates bonding curve PDA account (seeded with `["bonding_curve", mint]`) initialized with:
   - Virtual reserves set to global defaults
   - Real token reserves = total supply (all tokens minted to curve)
   - Real SOL reserves = 0
   - Stores creator's public key
   - `complete = false`
3. Mints entire supply to bonding curve's token account
4. Creates token metadata using Metaplex Token Metadata Program
5. Emits `TokenCreated` event with mint and creator addresses

**Accounts:**
- `signer`: Pays for token creation
- `creator`: Token creator
- `mint`: New token mint (PDA, seeded with `["mint", signer, unique_seed]`)
- `bonding_curve`: Bonding curve account (PDA)
- `bonding_curve_ata`: Bonding curve's associated token account

### 3. `buy`

Purchase tokens from the bonding curve using SOL. Price is determined by the constant product formula.

**Parameters:**
- `sol_amount`: Amount of SOL to spend
- `min_token_output`: Minimum tokens to receive (slippage protection)

**Logic:**

1. **Fee Calculation (Consistent for All Buys):**
   - `platform_fee = sol_amount * platform_trade_fee_bps / 10000`
   - `reserve_fee = sol_amount * reserve_trade_fee_bps / 10000`
   - `total_fee = platform_fee + reserve_fee`
   - `sol_after_fees = sol_amount - total_fee`

2. **User Token Purchase** (Constant Product):
   ```
   k = virtual_sol_reserves * virtual_token_reserves
   new_virtual_sol_reserves = virtual_sol_reserves + sol_after_fees
   new_virtual_token_reserves = k / new_virtual_sol_reserves
   user_tokens_out = virtual_token_reserves - new_virtual_token_reserves
   ```

3. **Reserve Token Purchase:**
   - Reserve fee (in SOL) is used to buy additional tokens from the curve
   - Uses the same constant product formula with updated reserves
   - Tokens are sent to the reserve's ATA for this token

4. **State Updates:**
   - Updates virtual reserves to reflect both purchases
   - Platform fee transferred to platform fee recipient
   - User receives their tokens
   - Reserve receives its tokens
   - Adds SOL to real reserves (user's SOL + reserve fee SOL, minus platform fee)

5. **Graduation Check:**
   - If `real_sol_reserves >= graduation_threshold`, sets `complete = true`
   - Emits `CurveComplete` event
   - Once complete, no more buys/sells are allowed

6. **Event Emission:**
   - Emits `Trade` event with `side: TradeSide::Buy`, trader, sol_amount, and token_amount

**Accounts:**
- `signer`: Buyer (pays SOL)
- `bonding_curve`: Curve account
- `user_ata`: Buyer's token account (created if needed)
- `reserve_ata`: Global reserve's token account for this token
- `platform_fee_recipient`: Receives platform fees

### 4. `sell`

Sell tokens back to the bonding curve and receive SOL.

**Parameters:**
- `token_amount`: Amount of tokens to sell
- `min_sol_output`: Minimum SOL to receive (slippage protection)

**Logic:**

1. **SOL Calculation** (Constant Product, reverse):
   ```
   k = virtual_sol_reserves * virtual_token_reserves
   new_virtual_token_reserves = virtual_token_reserves + token_amount
   new_virtual_sol_reserves = k / new_virtual_token_reserves
   sol_out_gross = virtual_sol_reserves - new_virtual_sol_reserves
   ```

2. **Fee Calculation:**
   - `platform_fee = sol_out_gross * platform_trade_fee_bps / 10000`
   - `reserve_fee = sol_out_gross * reserve_trade_fee_bps / 10000`
   - `total_fee = platform_fee + reserve_fee`
   - `sol_out_net = sol_out_gross - total_fee`

3. **Reserve Token Purchase:**
   - Reserve fee (in SOL) remains in the bonding curve and buys tokens
   - Uses the updated virtual reserves after the user's sell
   - Tokens are sent to the reserve's ATA for this token

4. **State Updates:**
   - Updates virtual reserves to reflect both the sell and reserve purchase
   - Platform fee transferred to platform fee recipient
   - User receives their net SOL
   - Reserve receives its tokens
   - Real SOL reserves decrease by `platform_fee + sol_out_net` (reserve fee stays in curve)

5. **Event Emission:**
   - Emits `Trade` event with `side: TradeSide::Sell`, trader, sol_amount, and token_amount

**Accounts:**
- `signer`: Seller (receives SOL)
- `bonding_curve`: Curve account
- `user_ata`: Seller's token account
- `reserve_ata`: Global reserve's token account for this token
- `platform_fee_recipient`: Receives platform fees

### 5. `withdraw`

Allows the program authority to withdraw tokens and SOL from graduated curves.

**Logic:**
- Transfers all tokens from bonding curve's token account to authority's token account
- Withdraws all SOL from bonding curve account (except rent-exempt minimum)

**Restrictions:**
- Only works on curves where `complete = true` (graduated)
- Only callable by the program authority

**Accounts:**
- `authority`: Program authority (must match global authority)
- `bonding_curve`: Graduated curve
- `authority_ata`: Authority's token account for receiving tokens

### 6. `withdraw_reserve`

Allows the program authority to withdraw tokens from the global reserve.

**Parameters:**
- `amount`: Amount of tokens to withdraw (supports partial withdrawals)

**Logic:**
- Transfers specified amount of tokens from reserve's ATA to authority's ATA
- Uses the global reserve PDA as the signing authority

**Restrictions:**
- Only callable by the program authority
- Requires `amount > 0`

**Accounts:**
- `authority`: Program authority (must match global authority)
- `global_reserve`: Global reserve PDA (authority for all reserve ATAs)
- `reserve_ata`: Reserve's token account for the specific token
- `authority_ata`: Authority's token account for receiving tokens

### 7. `deposit_to_reserve`

Allows the program authority to deposit tokens into the global reserve. Used for distributing fees collected off-chain (e.g., from Meteora liquidity pools).

**Parameters:**
- `amount`: Amount of tokens to deposit

**Logic:**
- Transfers specified amount of tokens from authority's ATA to reserve's ATA
- Standard token transfer signed by authority

**Restrictions:**
- Only callable by the program authority
- Requires `amount > 0`

**Use Case:**
This instruction enables a complete fee distribution cycle:
1. Graduated tokens are withdrawn and used to create Meteora liquidity pools
2. Trading fees accumulate in the pools
3. Fees are periodically withdrawn from Meteora by the authority
4. Fees are deposited back into the reserve using this instruction

**Accounts:**
- `authority`: Program authority (must match global authority)
- `authority_ata`: Authority's token account (source of tokens)
- `reserve_ata`: Reserve's token account (destination)

### 8. `update_global_config`

Updates the global configuration. Only callable by the current authority.

**Parameters:**
- All parameters from `initialize` (with `new_` prefix)

**Logic:**
- Validates authority
- Updates all global configuration values
- Validates that `new_platform_trade_fee_bps + new_reserve_trade_fee_bps <= 3000` (max 30%)

## Testing

### Setup Local Validator

Before running tests, start a local Solana validator with the Token Metadata program cloned:

```bash
solana-test-validator --reset \
  --clone metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s \
  --clone-upgradeable-program metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s \
  --url mainnet-beta
```

### Run Tests

In a separate terminal, run the Anchor tests:

```bash
anchor test --skip-local-validator
```

The tests cover:
- Token creation and bonding curve initialization with `TokenCreated` event
- Buy operations with consistent fee distribution (platform + reserve)
- Reserve token accumulation on both buys and sells
- Sell operations with comprehensive validation
- `Trade` events for both buy and sell operations
- Curve graduation and `CurveComplete` event emission
- Withdrawal from graduated curves (no cooldown)
- Reserve token withdrawals with partial amounts (no cooldown)
- Token deposits to reserve
- Global config updates with 30% fee cap validation
- Error conditions (insufficient SOL, slippage, fee cap exceeded, etc.)

## Client Integration

### Codama-Generated Clients

This project uses [Codama](https://github.com/codama-idl/codama) to generate multiple client types for interacting with the program:

- **JavaScript Client** (`clients/generated/js/`) - Compatible with [Solana Kit](https://github.com/anza-xyz/kit)
- **UMI Client** (`clients/generated/umi/`) - Compatible with [UMI](https://developers.metaplex.com/umi) (Metaplex framework) ⭐
- **Rust Client** (`clients/generated/rust/`) - For Rust/Solana SDK integration

**Recommended: UMI Client**

The UMI client is recommended for most integrations and provides:
- Type-safe instruction builders for all program instructions
- PDA derivation helpers
- Account fetch helpers
- Transaction building utilities

### Example Usage

The example file `clients/example.ts` uses the **UMI client** and demonstrates how to:

- Initialize the program with platform and reserve fees
- Create tokens with bonding curves
- Execute buys with consistent fee distribution
- Execute sells with reserve token purchases
- Update global configuration
- Withdraw from graduated curves
- Withdraw tokens from the global reserve
- Deposit tokens to the global reserve (Meteora fee distribution cycle)

**This example can be used as a reference when integrating the program into frontend or backend applications.**

**Key Concepts from the Example:**

1. **UMI Setup:**
   ```typescript
   const umi = createUmi('http://127.0.0.1:8899');
   umi.use(keypairIdentity(signer));
   ```

2. **PDA Derivation:**
   ```typescript
   // Global config PDA
   const globalPda = umi.eddsa.findPda(programId, [
     string({ size: 'variable' }).serialize('global')
   ])[0];
   
   // Global reserve PDA (authority for all reserve ATAs)
   const globalReservePda = umi.eddsa.findPda(programId, [
     string({ size: 'variable' }).serialize('reserve')
   ])[0];
   
   // Bonding curve PDA for a specific token
   const bondingCurvePda = umi.eddsa.findPda(programId, [
     string({ size: 'variable' }).serialize('bonding_curve'),
     publicKeySerializer().serialize(mint)
   ])[0];
   ```

3. **Instruction Calls:**
   ```typescript
   await initialize(umi, {
     authority: signer,
     initialVirtualTokenReserves: BigInt(1_073_000_191_000_000),
     initialVirtualSolReserves: BigInt(30) * LAMPORTS_PER_SOL,
     tokenTotalSupply: BigInt(1_000_000_000_000_000),
     platformTradeFeeBps: BigInt(100),  // 1%
     reserveTradeFeeBps: BigInt(400),   // 4%
     platformFeeRecipient: feeRecipient.publicKey,
     graduationThreshold: BigInt(2) * LAMPORTS_PER_SOL,
   }).sendAndConfirm(umi, { confirm: { commitment: 'processed' } });
   ```

4. **Associated Token Accounts:**
   The example shows how to derive ATAs and create them when needed (e.g., for withdraw operations).

### Running the Example

```bash
ts-node clients/example.ts
```

Make sure your local validator is running first!

## Program Architecture

### Accounts

- **Global**: Single PDA (seeded with `["global"]`) storing program-wide configuration
  - Stores authority, fee recipients, fee basis points, initial reserves, graduation threshold
  - References the global reserve PDA
- **GlobalReserve**: Single PDA (seeded with `["reserve"]`) acting as authority for all reserve token ATAs
  - Holds no data itself (minimal 8-byte account)
  - Used as signing authority for token transfers from reserve ATAs
- **BondingCurve**: One per token (seeded with `["bonding_curve", mint]`), stores curve state and creator address
  - Tracks virtual and real reserves
  - Stores completion status
  - Acts as SOL holder (via PDA lamports) and authority for the curve's token ATA

### Bonding Curve Mechanics

The bonding curve uses a constant product market maker (CPMM):
- **Virtual reserves**: Used for pricing calculations (inflated to ensure smooth price discovery)
- **Real reserves**: Actual SOL and tokens in the curve
- **Price formula**: `k = virtual_sol_reserves * virtual_token_reserves` (constant)
  - As more SOL is added, tokens become more expensive
  - As tokens are sold back, price decreases
- **Graduation**: When `real_sol_reserves >= graduation_threshold`, the curve completes
  - Emits `CurveComplete` event
  - No more trades allowed after graduation

### Fee Structure

All trades use **consistent percentage-based fees** split between:
- **Platform**: `platform_trade_fee_bps` basis points → Sent to `platform_fee_recipient`
- **Reserve**: `reserve_trade_fee_bps` basis points → Used to buy tokens from the curve and sent to reserve ATA

**Key Points:**
- No special first buy fee (removed for consistency)
- Reserve fee **always buys tokens** (on both buy and sell operations)
- Maximum total fees: 30% (`platform_trade_fee_bps + reserve_trade_fee_bps <= 3000`)
- Reserve accumulates tokens across all bonding curves in a single global reserve system

### Events

The program emits the following events for off-chain tracking:

1. **TokenCreated**: Emitted when a new token is created
   - `mint`: Token mint address
   - `creator`: Token creator address

2. **Trade**: Emitted on every buy or sell operation
   - `mint`: Token mint address
   - `trader`: Address executing the trade
   - `side`: `TradeSide` enum (`Buy` or `Sell`)
   - `sol_amount`: Amount of SOL involved in the trade
   - `token_amount`: Amount of tokens involved in the trade

3. **CurveComplete**: Emitted when a curve graduates
   - `mint`: Token mint address
   - `bonding_curve`: Bonding curve PDA address

## Development

### Building

```bash
anchor build
```

### Deploying

```bash
# Deploy to devnet
anchor deploy --provider.cluster devnet

# Deploy to localnet
anchor deploy
```

**Important: Program ID Synchronization**

After deploying, ensure your program ID is synchronized across all files:

1. **Generate a new program keypair** (if needed):
   ```bash
   solana-keygen new -o target/deploy/coinfun-keypair.json
   ```

2. **Sync the program ID** to `lib.rs` and `Anchor.toml`:
   ```bash
   anchor keys sync
   ```

3. **Rebuild** with the correct program ID:
   ```bash
   anchor build
   ```

4. **Redeploy** to ensure the deployed program matches the declared ID:
   ```bash
   anchor deploy --provider.cluster devnet
   ```

If you see a `DeclaredProgramIdMismatch` error, it means the `declare_id!` in `lib.rs` doesn't match the deployed program. Follow the steps above to fix it.

### Program ID

The program ID is declared in `programs/coinfun/src/lib.rs` using the `declare_id!` macro

#### Using a Different Program ID

When you deploy the program with a new address, the generated client will still reference the original program ID. To use your new program ID with the UMI client:

1. **Register the custom program ID with UMI** before using any instructions:

```typescript
import { publicKey } from "@metaplex-foundation/umi";
import { createCoinfunProgram } from "./generated/umi/src/programs/coinfun";

const CUSTOM_PROGRAM_ID = publicKey("YOUR_NEW_PROGRAM_ID_HERE");

// Option 1: Using the helper function
const customProgram = createCoinfunProgram();
customProgram.publicKey = CUSTOM_PROGRAM_ID;
umi.programs.add(customProgram);

// Option 2: Direct registration
umi.programs.add({
  name: 'coinfun',
  publicKey: CUSTOM_PROGRAM_ID,
});
```

2. **All instructions will automatically use the registered program ID**. This means:
   - PDA derivations will use the new program ID
   - All instruction calls will target the new program
   - No need to modify the generated client code

**Important Notes:**
- The program ID is hardcoded in the generated client (`clients/generated/umi/src/programs/coinfun.ts`)
- Registering a custom program ID overrides the default for that UMI instance
- If you need to support multiple program IDs, you can register different ones per UMI instance
- After deploying, ensure your program's `declare_id!` macro in `lib.rs` matches the deployed program ID

See `clients/example.ts` for a commented example of how to register a custom program ID.

## Complete Token Lifecycle

Understanding the full lifecycle from token creation to DEX liquidity:

### Phase 1: Bonding Curve Trading

1. **Token Creation**: Creator calls `create` instruction
   - New SPL token mint created with 6 decimals
   - Bonding curve initialized with virtual reserves
   - Total supply minted to bonding curve's ATA
   - `TokenCreated` event emitted

2. **Trading Period**: Users buy and sell on the bonding curve
   - Each trade emits a `Trade` event (`Buy` or `Sell`)
   - Platform fees sent to `platform_fee_recipient`
   - Reserve fees automatically buy tokens → sent to reserve ATA
   - Reserve accumulates tokens from all trading activity
   - Real SOL reserves increase toward graduation threshold

3. **Graduation**: When `real_sol_reserves >= graduation_threshold`
   - `complete = true` set on bonding curve
   - `CurveComplete` event emitted
   - No more trades allowed on this curve

### Phase 2: DEX Liquidity (Off-Chain)

4. **Withdrawal**: Authority calls `withdraw` instruction
   - All tokens withdrawn from bonding curve's ATA
   - All SOL withdrawn from bonding curve (minus rent)
   - Authority now holds the graduated token supply and SOL

5. **Reserve Withdrawal**: Authority calls `withdraw_reserve` instruction
   - Withdraws accumulated tokens from the global reserve
   - Can specify partial amounts for controlled liquidity management

6. **Liquidity Pool Creation**: Authority creates Meteora DAMM v2 pool
   - Uses withdrawn tokens and SOL
   - Pool starts generating trading fees off-chain

### Phase 3: Fee Distribution (Off-Chain to On-Chain)

7. **Fee Collection**: Off-chain cronjob monitors Meteora pools
   - Periodically withdraws accumulated trading fees
   - Fees deposited to authority's wallet

8. **Fee Redistribution**: Authority calls `deposit_to_reserve` instruction
   - Transfers collected fees from authority's ATA to reserve ATA
   - Closes the loop: on-chain fees → DEX → back to on-chain reserve
   - Reserve continues to grow from both on-chain trades and DEX fees

### Key Benefits of This Architecture

- **Unified Reserve**: Single global reserve PDA manages all token reserves efficiently
- **Continuous Fee Flow**: Reserve accumulates tokens during bonding curve phase AND after DEX graduation
- **Flexible Withdrawals**: Partial withdrawals allow gradual liquidity management
- **Event-Driven**: All major actions emit events for easy off-chain tracking and automation
- **No Cooldowns**: Admin operations (withdraw, withdraw_reserve) have no cooldowns for operational flexibility
