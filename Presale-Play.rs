use anchor_lang::prelude::*;
#[allow(unused_imports)]
use anchor_lang::solana_program::program_error::ProgramError;
use pyth_sdk_solana::load_price_feed_from_account_info;
use solana_program::entrypoint::ProgramResult;
// use anchor_spl::token_interface::{Transfer, TokenAccount};
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("ACAzRjWNaiDHnVRUKYXz2PHSNPFNmLrpKCjAAcvJt1va");

#[program]
pub mod fam_presale_contract {
    use super::*;

    // Initialize presale and vesting parameters
    pub fn initialize(
        ctx: Context<Initialize>,
        presale_start: i64,
        presale_end: i64,
        public_sale_start: i64,
        price: u64,
        max_allocation: u64,
        cliff_period: i64,
        vesting_period: i64,
        vesting_interval: i64,
        airdrop_percentages: Vec<u64>, // Accept airdrop percentages as input
        max_airdrop_elements: u8,      // Accept maximum airdrop elements as input
    ) -> ProgramResult {
        // Cap the size of the airdrop_percentages vector (e.g., max 12 elements)
        if airdrop_percentages.len() > max_airdrop_elements.into() {
            return Err(ErrorCode::AirdropConfigurationError.into());
        }
        let total_percentage: u64 = airdrop_percentages.iter().sum();
        if total_percentage > 100 {
            return Err(ErrorCode::AirdropConfigurationError.into());
        }
        if vesting_period == 0 || vesting_interval == 0 {
            return Err(ErrorCode::InvalidVestingParameters.into());
        }
        if vesting_interval > vesting_period {
            return Err(ErrorCode::InvalidVestingParameters.into());
        }
        if cliff_period > vesting_period {
            return Err(ErrorCode::InvalidVestingParameters.into());
        }
        if presale_start >= presale_end {
            return Err(ErrorCode::InvalidPresaleTiming.into());
        }
        if public_sale_start <= presale_end {
            return Err(ErrorCode::InvalidPresaleTiming.into());
        }
        if airdrop_percentages.is_empty() {
            return Err(ErrorCode::AirdropConfigurationError.into());
        }
        if airdrop_percentages.iter().any(|&x| x == 0) {
            return Err(ErrorCode::AirdropConfigurationError.into());
        }

        let presale_account = &mut ctx.accounts.presale_account;
        presale_account.presale_start = presale_start;
        presale_account.presale_end = presale_end;
        presale_account.public_sale_start = public_sale_start;
        presale_account.price = price;
        presale_account.max_allocation = max_allocation;
        presale_account.cliff_period = cliff_period;
        presale_account.vesting_period = vesting_period;
        presale_account.vesting_interval = vesting_interval;
        presale_account.airdrop_percentages = airdrop_percentages;

        Ok(())
    }

    pub fn update_presale_params(
        ctx: Context<UpdatePresaleParams>,
        new_price: Option<u64>,
        new_min_buy_amount: Option<u64>,
        new_max_buy_amount: Option<u64>,
        new_hard_cap: Option<u64>,
    ) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;

        // Ensure caller is the authorized admin
        if ctx.accounts.authority.key() != presale_account.authority {
            return Err(ErrorCode::UnauthorizedAccess.into());
        }

        // Update parameters if provided
        if let Some(price) = new_price {
            presale_account.price = price;
        }
        if let Some(min_buy) = new_min_buy_amount {
            presale_account.min_buy_amount_sol = min_buy;
        }
        if let Some(max_buy) = new_max_buy_amount {
            presale_account.max_buy_amount_sol = max_buy;
        }
        if let Some(hard_cap) = new_hard_cap {
            presale_account.hard_cap_sol = hard_cap;
        }

        // Emit event with updated parameters
        let clock = Clock::get()?;
        emit!(PresaleParamsUpdated {
            new_price,
            new_min_buy_amount,
            new_max_buy_amount,
            new_hard_cap,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    pub fn purchase(ctx: Context<Purchase>, amount: u64) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;
        let user_vesting = &mut ctx.accounts.user_vesting;

        // Fetch current timestamp
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure presale is not paused
        if presale_account.paused {
            return Err(ErrorCode::PresalePaused.into());
        }

        // Ensure presale is active
        if !(current_time >= presale_account.presale_start
            && current_time <= presale_account.presale_end)
        {
            return Err(ErrorCode::SaleNotActive.into());
        }

        // Fetch SOL/USD price using fallback logic
        let sol_price_in_usd = get_price_from_oracle(
            &ctx.accounts.sol_to_usd_oracle,
            presale_account.manual_price_override,
        )?;

        // Calculate the token price in SOL
        let token_price_in_sol = presale_account
            .price
            .checked_mul(10u64.pow(9)) // Convert price from USD cents to lamports
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(sol_price_in_usd)
            .ok_or(ErrorCode::MathOverflow)?;

        // Calculate total cost in SOL (lamports)
        let total_cost_in_sol = amount
            .checked_mul(token_price_in_sol)
            .ok_or(ErrorCode::MathOverflow)?;

        // Ensure the buyer has enough SOL
        if **ctx.accounts.buyer.to_account_info().lamports.borrow() < total_cost_in_sol {
            return Err(ErrorCode::InsufficientFunds.into());
        }

        // Ensure the purchase is within allowed limits
        if total_cost_in_sol < presale_account.min_buy_amount_sol {
            return Err(ErrorCode::BelowMinimumPurchase.into());
        }
        if user_vesting
            .total_purchased_sol
            .checked_add(total_cost_in_sol)
            .ok_or(ErrorCode::MathOverflow)?
            > presale_account.max_buy_amount_sol
        {
            return Err(ErrorCode::ExceedsMaximumPurchase.into());
        }

        // Check if the total sold exceeds the global hard cap
        if presale_account
            .total_sold_sol
            .checked_add(total_cost_in_sol)
            .ok_or(ErrorCode::MathOverflow)?
            > presale_account.hard_cap_sol
        {
            return Err(ErrorCode::HardCapReached.into());
        }

        // --- STATE UPDATES ---
        presale_account.total_sold_sol = presale_account
            .total_sold_sol
            .checked_add(total_cost_in_sol)
            .ok_or(ErrorCode::BadMath)?;

        user_vesting.total_amount = user_vesting
            .total_amount
            .checked_add(amount)
            .ok_or(ErrorCode::BadMath)?;
        user_vesting.total_purchased_sol = user_vesting
            .total_purchased_sol
            .checked_add(total_cost_in_sol)
            .ok_or(ErrorCode::BadMath)?;

        // Ensure the amount is non-zero
        if amount == 0 || total_cost_in_sol < presale_account.min_buy_amount_sol {
            return Err(ErrorCode::BelowMinimumPurchase.into());
        }

        // Derive a minimum token purchase based on `min_buy_amount_sol` and token price
        let min_token_purchase = presale_account
            .min_buy_amount_sol
            .checked_mul(sol_price_in_usd)
            .ok_or(ErrorCode::BadMath)?
            .checked_div(presale_account.price)
            .ok_or(ErrorCode::BadMath)?;

        if amount < min_token_purchase {
            return Err(ErrorCode::BelowMinimumPurchase.into());
        }

        // --- EXTERNAL CALL ---
        let program_pda = ctx.accounts.presale_account.to_account_info().key;
        **ctx
            .accounts
            .buyer
            .to_account_info()
            .try_borrow_mut_lamports()? -= total_cost_in_sol;
        **program_pda.try_borrow_mut_lamports()? += total_cost_in_sol;

        // Emit event
        emit!(PurchaseEvent {
            buyer: ctx.accounts.buyer.key(),
            amount,
            cost_in_sol: total_cost_in_sol,
            timestamp: current_time,
        });

        Ok(())
    }

    pub fn set_pause_state(ctx: Context<SetPauseState>, paused: bool) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;

        // Ensure caller is the authorized admin
        if ctx.accounts.authority.key() != presale_account.authority {
            return Err(ErrorCode::UnauthorizedAccess.into());
        }

        // Update pause state
        presale_account.paused = paused;

        // Emit pause state change event
        let clock = Clock::get()?;
        emit!(PauseStateChanged {
            paused,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    // Batch airdrop distribution to save compute units
    const MAX_BATCH_SIZE: usize = 50; // Set a limit for batch size

    pub fn distribute_airdrops_batch(
        ctx: Context<BatchDistributeAirdrops>,
        users: Vec<UserDistribution>,
    ) -> ProgramResult {
        const MAX_BATCH_SIZE: usize = 50; // Set a limit for batch size

        let presale_account = &ctx.accounts.presale_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure presale has ended
        if current_time < presale_account.presale_end {
            return Err(ErrorCode::PresaleNotEnded.into());
        }

        // Ensure batch size does not exceed MAX_BATCH_SIZE
        if users.len() > MAX_BATCH_SIZE {
            return Err(ErrorCode::BatchTooLarge.into());
        }

        // Iterate over users and process airdrops
        for user in users.iter() {
            // Validate that the user_vesting_index is within bounds
            if user.user_vesting_index >= ctx.remaining_accounts.len() {
                return Err(ErrorCode::InvalidUserAccountIndex.into());
            }

            // Safely load the user vesting account
            let user_vesting_account = &mut Account::<UserVesting>::try_from(
                &ctx.remaining_accounts[user.user_vesting_index],
            )?;

            // Skip users who have completed all their airdrops
            if user_vesting_account.airdrops_completed >= presale_account.total_airdrop_periods {
                continue;
            }

            // Ensure airdrop index is valid
            let airdrop_percentage = presale_account
                .airdrop_percentages
                .get(user.airdrop_index as usize)
                .ok_or(ErrorCode::AirdropConfigurationError)?;

            // Calculate the airdrop amount
            let airdrop_amount = user_vesting_account
                .total_amount
                .checked_mul(*airdrop_percentage as u64)
                .ok_or(ErrorCode::MathOverflow)?
                .checked_div(100)
                .ok_or(ErrorCode::MathOverflow)?;
            // Transfer the airdrop tokens
            token::transfer(
                ctx.accounts.into_transfer_context(user_vesting_account),
                airdrop_amount,
            )?;
            if airdrop_amount == 0 {
                continue; // Avoid unnecessary transfers or updates
            }
            // Update user vesting account
            user_vesting_account.claimed_amount = user_vesting_account
                .claimed_amount
                .checked_add(airdrop_amount)
                .ok_or(ErrorCode::MathOverflow)?;
            user_vesting_account.airdrops_completed = user_vesting_account
                .airdrops_completed
                .checked_add(1)
                .ok_or(ErrorCode::MathOverflow)?;
        }

        Ok(())
    }

    pub fn refund(ctx: Context<Refund>, refund_amount: u64) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;
        let user_vesting = &mut ctx.accounts.user_vesting;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure the refund period is active
        if current_time < presale_account.presale_end {
            return Err(ErrorCode::RefundNotAvailable.into());
        }

        // Calculate claimable and refundable tokens
        let claimable_tokens = calculate_vested_amount(
            user_vesting.total_amount,
            user_vesting.start_time,
            presale_account.vesting_period,
            presale_account.vesting_interval,
            current_time,
        ).saturating_sub(user_vesting.claimed_amount);

        let refundable_tokens = user_vesting
            .total_amount
            .saturating_sub(claimable_tokens)
            .saturating_sub(user_vesting.claimed_amount);

        if refund_amount == 0 || refundable_tokens == 0 {
            return Err(ErrorCode::InsufficientRefundBalance.into());
        }

        // Fetch SOL/USD price using fallback logic
        let sol_price_in_usd = get_price_from_oracle(
            &ctx.accounts.sol_to_usd_oracle,
            presale_account.manual_price_override,
        )?;

        // Calculate refund amount in SOL
        let refund_sol =
            calculate_sol_price(refund_amount, presale_account.price, sol_price_in_usd)?;

        // Ensure the program PDA has enough SOL for the refund
        let program_pda = ctx.accounts.presale_account.to_account_info();
        if **program_pda.lamports.borrow() < refund_sol {
            return Err(ErrorCode::InsufficientProgramBalance.into());
        }

        // Process the refund
        **program_pda.try_borrow_mut_lamports()? -= refund_sol;
        **ctx
            .accounts
            .buyer
            .to_account_info()
            .try_borrow_mut_lamports()? += refund_sol;

        // Update metrics
        user_vesting.total_amount = user_vesting
            .total_amount
            .checked_sub(refund_amount)
            .ok_or(ErrorCode::MathOverflow)?;
        presale_account.total_sold_sol = presale_account
            .total_sold_sol
            .checked_sub(refund_amount)
            .ok_or(ErrorCode::MathOverflow)?;

        // Emit event
        emit!(RefundEvent {
            buyer: ctx.accounts.buyer.key(),
            refund_amount,
            refund_sol,
            remaining_tokens: refundable_tokens - refund_amount,
            total_refund_tokens: user_vesting.total_amount,
            total_refunded_sol: presale_account.total_sold_sol,
        });

        Ok(())
    }

    pub fn update_manual_price_override(
        ctx: Context<UpdateManualPriceOverride>,
        new_price: Option<u64>,
    ) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;

        // Ensure caller is the authorized admin
        if ctx.accounts.authority.key() != presale_account.authority {
            return Err(ErrorCode::UnauthorizedAccess.into());
        }

        // Validate the manual price override
        if let Some(price) = new_price {
            if price == 0 || price > 1_000_000 {
                // Ensure price is non-zero and within reasonable bounds
                return Err(ErrorCode::InvalidPrice.into());
            }
        }

        // Update the manual price override
        presale_account.manual_price_override = new_price;

        // Emit event
        let clock = Clock::get()?;
        emit!(ManualPriceOverrideUpdated {
            new_price,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    pub fn calculate_claimable(ctx: Context<CalculateClaimable>) -> Result<u64> {
        let user_vesting = &ctx.accounts.user_vesting;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        let vested_amount = calculate_vested_amount(
            user_vesting.total_amount,
            user_vesting.start_time,
            ctx.accounts.presale_account.vesting_period,
            ctx.accounts.presale_account.vesting_interval,
            current_time,
        );

        // Return the amount of claimable tokens
        Ok(vested_amount.saturating_sub(user_vesting.claimed_amount))
    }

    impl<'info> Claim<'info> {
        fn into_transfer_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
            CpiContext::new(
                self.token_program.to_account_info(),
                Transfer {
                    from: self.user_vesting.to_account_info(),
                    to: self.buyer.to_account_info(),
                    authority: self.buyer.to_account_info(),
                },
            )
        }
    }

    pub fn update_presale_discount(
        ctx: Context<UpdatePresaleParams>,
        new_price: Option<u64>,
    ) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;

        // Validate and update price directly
        if let Some(price) = new_price {
            if price == 0 {
                return Err(ErrorCode::InvalidPrice.into());
            }
            presale_account.price = price;
        }

        Ok(())
    }

    pub fn distribute_initial_airdrop(ctx: Context<DistributeAirdrop>) -> ProgramResult {
        let user_vesting = &mut ctx.accounts.user_vesting;
        let presale_account = &ctx.accounts.presale_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure presale has ended
        if current_time < presale_account.presale_end {
            return Err(ErrorCode::PresaleNotEnded.into());
        }

        // Calculate initial airdrop percentage
        let initial_percentage = *presale_account
            .airdrop_percentages
            .get(0)
            .ok_or(ErrorCode::AirdropConfigurationError)?;
        let initial_airdrop = user_vesting
            .total_amount
            .checked_mul(initial_percentage)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(100)
            .ok_or(ErrorCode::MathOverflow)?;

        // Transfer initial airdrop
        token::transfer(ctx.accounts.into_transfer_context(), initial_airdrop)?;

        // Update claimed amount and airdrop count
        user_vesting.claimed_amount += initial_airdrop;
        user_vesting.airdrops_completed = 1;

        Ok(())
    }

    pub fn distribute_monthly_airdrop(ctx: Context<DistributeAirdrop>) -> ProgramResult {
        let user_vesting = &mut ctx.accounts.user_vesting;
        let presale_account = &ctx.accounts.presale_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure at least one month has passed since the last airdrop
        let months_elapsed =
            (current_time - user_vesting.start_time) / presale_account.vesting_interval;
        if months_elapsed as u8 <= user_vesting.airdrops_completed {
            return Err(ErrorCode::AirdropNotDue.into());
        }

        // Ensure airdrops do not exceed total periods
        if user_vesting.airdrops_completed >= presale_account.total_airdrop_periods {
            return Err(ErrorCode::AirdropCompleted.into());
        }

        // Get the percentage for the current airdrop
        let current_percentage = *presale_account
            .airdrop_percentages
            .get(user_vesting.airdrops_completed as usize)
            .ok_or(ErrorCode::AirdropConfigurationError)?;

        // Calculate the airdrop amount
        let airdrop_amount = user_vesting
            .total_amount
            .checked_mul(current_percentage)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(100)
            .ok_or(ErrorCode::MathOverflow)?;

        // Transfer the airdrop amount
        token::transfer(ctx.accounts.into_transfer_context(), airdrop_amount)?;

        // Update claimed amount and airdrop count
        user_vesting.claimed_amount += airdrop_amount;
        user_vesting.airdrops_completed += 1;

        Ok(())
    }
}

// Utility function: Place this outside the #[program] module
pub fn derive_presale_pda(program_id: &Pubkey, seed: &[u8]) -> Pubkey {
    Pubkey::find_program_address(&[seed], program_id).0
}

pub fn calculate_sol_price(
    amount: u64,
    price: u64,
    sol_price_in_usd: u64,
) -> Result<u64> {
    amount
        .checked_mul(price)
        .ok_or(ErrorCode::MathOverflow)?
        .checked_mul(10u64.pow(9)) // Convert USD cents to lamports
        .ok_or(ErrorCode::MathOverflow)?
        .checked_div(sol_price_in_usd)
        .ok_or(ErrorCode::MathOverflow)
}

pub fn get_price_from_oracle(
    oracle_account: &AccountInfo,
    manual_price_override: Option<u64>,
) -> Result<u64> {
    // Attempt to load the price feed from the oracle
    if let Ok(price_feed) = load_price_feed_from_account_info(oracle_account) {
        if let Some(price_data) = price_feed.get_current_price() {
            if price_data.price > 0 {
                return Ok(price_data.price as u64); // Return valid oracle price
            }
        }
    }

    // Fallback to manual price override if oracle fails or returns invalid data
    if let Some(price) = manual_price_override {
        if price == 0 || price > 1_000_000 {
            // Validate fallback price
            return Err(ErrorCode::InvalidPrice.into());
        }
        return Ok(price);
    }

    Err(ErrorCode::PriceFeedUnavailable.into())
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)] // Mark the payer account as mutable
    pub user: Signer<'info>,
    #[account(init, payer = user, space = 8 + 64)] // Payer = user
    pub data_account: Account<'info, SomeData>,
    pub system_program: Program<'info, System>, // No need to mark this as mutable
}

// Example account struct
#[account]
pub struct DataAccount {
    pub data: u64,
}

#[account]
pub struct SomeData {
    pub value: String, // Store unsupported fields inside program-defined accounts
    pub extra_data: [u8; 56], // 56 bytes
}

#[account]
pub struct UserVesting {
    pub total_amount: u64,        // Total tokens purchased
    pub claimed_amount: u64,      // Tokens already claimed
    pub start_time: i64,          // Presale end time
    pub airdrops_completed: u8,   // Number of airdrops already distributed
    pub total_purchased_sol: u64, // Total SOL equivalent purchased by this user
}

#[account]
pub struct PresaleAccount {
    pub presale_start: i64,
    pub presale_end: i64,
    pub public_sale_start: i64,
    pub price: u64, // Price per token in USD cents
    pub max_allocation: u64,
    pub cliff_period: i64,
    pub vesting_period: i64,
    pub vesting_interval: i64,
    pub total_airdrop_periods: u8,
    pub airdrop_percentages: Vec<u8>,
    pub total_sold_sol: u64,                // Total tokens sold
    pub min_buy_amount_sol: u64,            // Minimum SOL amount per purchase
    pub max_buy_amount_sol: u64,            // Maximum SOL amount per user
    pub hard_cap_sol: u64,                  // Maximum SOL for the entire presale
    pub authority: Pubkey,                  // Admin authority key
    pub manual_price_override: Option<u64>, // Optional manual price in USD cents
    pub paused: bool,                       // Whether the presale is paused
}

#[derive(Accounts)]
pub struct CalculateClaimable<'info> {
    #[account(mut)]
    pub user_vesting: Account<'info, UserVesting>,
    pub presale_account: Account<'info, PresaleAccount>,
}

#[derive(Accounts)]
pub struct UpdateManualPriceOverride<'info> {
    #[account(mut, has_one = authority)]
    pub presale_account: Account<'info, PresaleAccount>,
    pub authority: Signer<'info>, // Admin account
}

#[derive(Accounts)]
pub struct UpdatePresaleParams<'info> {
    #[account(mut, has_one = authority)]
    pub presale_account: Account<'info, PresaleAccount>,
    pub authority: Signer<'info>, // Admin account
}

// Define the `Claim` context for claiming tokens
#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub user_vesting: Account<'info, UserVesting>,
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Added
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct DistributeAirdrop<'info> {
    #[account(mut)]
    pub user_vesting: Account<'info, UserVesting>,
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Purchase<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>,
    #[account(mut)]
    pub user_vesting: Account<'info, UserVesting>,
    #[account(mut)]
    pub buyer: Signer<'info>,
    pub sol_to_usd_oracle: AccountInfo<'info>, // Oracle account for SOL/USD price
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale account storing presale details
    #[account(mut)]
    pub user_vesting: Account<'info, UserVesting>, // User's vesting account
    #[account(mut)]
    pub buyer: Signer<'info>, // User requesting the refund
    pub sol_to_usd_oracle: AccountInfo<'info>, // Oracle account for SOL/USD price
    pub system_program: Program<'info, System>, // System program for SOL transfers
}

#[derive(Accounts)]
pub struct SetPauseState<'info> {
    #[account(mut, has_one = authority)]
    pub presale_account: Account<'info, PresaleAccount>,
    pub authority: Signer<'info>, // Admin account
}

#[derive(Accounts)]
pub struct BatchDistributeAirdrops<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,               // The account that signs the transaction
    pub system_program: Program<'info, System>, // Required system program
}

#[derive(AnchorDeserialize, AnchorSerialize, Clone)]
pub struct UserDistribution {
    pub user_vesting_index: usize, // Index in the remaining accounts array
    pub airdrop_index: u8,         // Index of the current airdrop percentage
}

// Define the PurchaseEvent at the top of your contract
#[event]
pub struct PurchaseEvent {
    pub buyer: Pubkey,            // Buyer's wallet public key
    pub amount: u64,              // Number of tokens purchased
    pub cost_in_sol: Option<u64>, // Cost in SOL equivalent (in lamports, if applicable)
    pub timestamp: i64,
}

// Utility function to derive the program's PDA
#[event]
pub struct PauseStateChanged {
    pub paused: bool, // Whether the presale is paused
    pub timestamp: i64,
}

#[event]
pub struct PresaleParamsUpdated {
    pub new_price: Option<u64>,
    pub new_min_buy_amount: Option<u64>,
    pub new_max_buy_amount: Option<u64>,
    pub new_hard_cap: Option<u64>,
    pub timestamp: i64,
}

#[event]
pub struct RefundEvent {
    pub buyer: Pubkey,            // User's wallet public key
    pub refund_amount: u64,       // Number of tokens refunded
    pub refund_sol: u64,          // Amount of SOL refunded
    pub remaining_tokens: u64,    // Remaining refundable tokens
    pub total_refund_tokens: u64, // Total tokens refunded so far
    pub total_refunded_sol: u64,  // Total SOL refunded so far
}

#[event]
pub struct ClaimEvent {
    pub user: Pubkey,       // User's public key
    pub amount: u64,        // Amount of tokens claimed
    pub total_claimed: u64, // Total claimed tokens after this transaction
}

#[event]
pub struct ManualPriceOverrideUpdated {
    pub new_price: Option<u64>, // Updated manual price
    pub timestamp: i64,         // Time of the update
}

pub fn claim(ctx: Context<Claim>) -> ProgramResult {
    let user_vesting = &mut ctx.accounts.user_vesting;
    let clock = Clock::get()?;
    let current_time = clock.unix_timestamp;

    // Calculate vested tokens
    let vested_amount = calculate_vested_amount(
        user_vesting.total_amount,
        user_vesting.start_time,
        ctx.accounts.presale_account.vesting_period,
        ctx.accounts.presale_account.vesting_interval,
        current_time,
    );

    let claimable_amount = vested_amount.saturating_sub(user_vesting.claimed_amount);
    if claimable_amount > 0 {
        token::transfer(ctx.accounts.into_transfer_context(), claimable_amount)?;

        // Update claimed amount
        user_vesting.claimed_amount += claimable_amount;

        // Emit event
        emit!(ClaimEvent {
            user: ctx.accounts.buyer.key(),
            amount: claimable_amount,
            total_claimed: user_vesting.claimed_amount,
        });

        Ok(())
    } else {
        Err(ErrorCode::NoTokensToClaim.into())
    }
}

pub fn distribute_airdrops_batch(ctx: Context<BatchDistributeAirdrops>, amount: u64) -> Result<()> {
    // Iterate through the dynamic accounts in `remaining_accounts`
    for account_info in ctx.remaining_accounts.iter() {
        // Validate each account as a TokenAccount
        let recipient: Account<TokenAccount> = Account::try_from(account_info)?;

        // Log the recipient for debugging purposes
        msg!(
            "Distributing {} tokens to recipient {}",
            amount,
            recipient.key()
        );
    }

    Ok(())
}

// Helper to calculate vested amount
pub fn calculate_vested_amount(
    total_amount: u64,
    start_time: i64,
    vesting_period: i64,
    vesting_interval: i64,
    current_time: i64,
) -> u64 {
    if current_time < start_time {
        return 0;
    }
    let elapsed_time = current_time - start_time;

    if vesting_period == 0 || vesting_interval == 0 {
        return 0;
    }

    let vested_intervals = elapsed_time / vesting_interval;
    let total_intervals = vesting_period / vesting_interval;

    if total_intervals == 0 {
        return 0;
    }

    // Calculate the proportion of vested tokens
    let vested_amount =
        (total_amount as u128 * vested_intervals as u128 / total_intervals as u128) as u64;
    vested_amount
}

#[error_code]
pub enum ErrorCode {
    #[msg("The sale is not currently active.")]
    SaleNotActive,
    #[msg("No tokens available for claiming.")]
    NoTokensToClaim,
    #[msg("The presale has not ended.")]
    PresaleNotEnded,
    #[msg("Refunds are not available.")]
    RefundNotAvailable,
    #[msg("Insufficient unclaimed tokens for refund.")]
    InsufficientRefundBalance,
    #[msg("Program does not have enough SOL for the refund.")]
    InsufficientProgramBalance,
    #[msg("All airdrops have been completed.")]
    AirdropCompleted,
    #[msg("Invalid vesting parameters.")]
    InvalidVestingParameters,
    #[msg("Purchase amount exceeds maximum allocation.")]
    AllocationExceeded,
    #[msg("Insufficient funds for purchase.")]
    InsufficientFunds,
    #[msg("Math overflow occurred.")]
    MathOverflow,
    #[msg("Math overflow occurred.")]
    BadMath,
    #[msg("Invalid payment method.")]
    InvalidPaymentMethod,
    #[msg("Price feed is unavailable.")]
    PriceFeedUnavailable,
    #[msg("Airdrop configuration error.")]
    AirdropConfigurationError,
    #[msg("Purchase amount is below the minimum buy amount.")]
    BelowMinimumPurchase,
    #[msg("Purchase amount exceeds the maximum allowed for this user.")]
    ExceedsMaximumPurchase,
    #[msg("Presale hard cap has been reached.")]
    HardCapReached,
    #[msg("Unauthorized access.")]
    UnauthorizedAccess,
    #[msg("Invalid parameter value.")]
    InvalidParameterValue,
    #[msg("Invalid discount percentage. Must be <= 100.")]
    InvalidDiscountPercentage,
    #[msg("Invalid price. Price must be greater than zero.")]
    InvalidPrice,
    #[msg("Batch size exceeds the maximum limit.")]
    BatchTooLarge,
    #[msg("Invalid user account index.")]
    InvalidUserAccountIndex,
    #[msg("Presale is currently paused.")]
    PresalePaused,
}
