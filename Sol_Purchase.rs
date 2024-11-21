use anchor_lang::prelude::*;
use pyth_sdk_solana::{load_price_feed_from_account_info, PriceFeed};
use pyth_sol_sdk::price_update::PriceUpdateV2;
use arrayref::array_ref;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("13WjtSt6dp9qQFrvcx1ncD2gHSyhNMAqwEqwQkSgpmya");

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
        max_airdrop_elements: u8       // Accept maximum airdrop elements as input
    ) -> ProgramResult {
        // Cap the size of the airdrop_percentages vector (e.g., max 12 elements)
        if airdrop_percentages.len() > max_airdrop_elements.into() {
            return Err(ErrorCode::AirdropConfigurationError.into());
        }
    
        if vesting_period == 0 || vesting_interval == 0 {
            return Err(ErrorCode::InvalidVestingParameters.into());
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
        new_discount_percentage: Option<u64>,
        new_min_buy_amount: Option<u64>,
        new_max_buy_amount: Option<u64>,
        new_hard_cap: Option<u64>,
    ) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;
    
        // Update parameters if provided
        if let Some(price) = new_price {
            presale_account.price = price;
        }
        if let Some(discount) = new_discount_percentage {
            presale_account.discount_percentage = discount;
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
    
        Ok(())
    }

    #[account]
    pub struct UserVesting {
        pub total_amount: u64,           // Total tokens purchased
        pub claimed_amount: u64,         // Tokens already claimed
        pub start_time: i64,             // Presale end time
        pub airdrops_completed: u8,      // Number of airdrops already distributed
        pub total_purchased_sol: u64,    // Total SOL equivalent purchased by this user
    }

    // Define the PurchaseEvent at the top of your contract
    #[event]
    pub struct PurchaseEvent {
        pub buyer: Pubkey,             // Buyer's wallet public key
        pub amount: u64,               // Number of tokens purchased
        pub cost_in_sol: Option<u64>,  // Cost in SOL equivalent (in lamports, if applicable)
    }

    pub fn purchase(
        ctx: Context<Purchase>, 
        amount: u64, 
        precomputed_cost: u64
    ) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;
        let user_vesting = &mut ctx.accounts.user_vesting;
    
        // Fetch current timestamp
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
    
        // Ensure presale is active
        if !(current_time >= presale_account.presale_start && current_time <= presale_account.presale_end) {
            return Err(ErrorCode::SaleNotActive.into());
        }
    
        // Validate the oracle feed for SOL/USDC price
        validate_oracle_feed(&ctx.accounts.sol_to_usdc_feed)?;
    
        // Fetch the SOL to USDC conversion rate from the oracle
        let sol_to_usdc_rate = get_sol_to_usdc_rate(&ctx.accounts.sol_to_usdc_feed)?;
    
        // Calculate the discounted token price
        let discounted_price = presale_account
            .price
            .checked_mul(100 - presale_account.discount_percentage)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(100)
            .ok_or(ErrorCode::MathOverflow)?;
    
        // Calculate the total cost in USDC
        let total_cost_in_usdc = amount
            .checked_mul(discounted_price)
            .ok_or(ErrorCode::MathOverflow)?;
    
        // Verify the precomputed cost passed by the client matches the on-chain calculation
        if total_cost_in_usdc != precomputed_cost {
            return Err(ErrorCode::InvalidPrecomputedCost.into());
        }
    
        // Calculate the total cost in SOL equivalent
        let total_cost_in_sol = total_cost_in_usdc
            .checked_mul(10u64.pow(9)) // Convert to lamports (1 SOL = 10^9 lamports)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(sol_to_usdc_rate)
            .ok_or(ErrorCode::MathOverflow)?;
    
        // Ensure the purchase is within allowed limits
        if total_cost_in_usdc < presale_account.min_buy_amount_sol {
            return Err(ErrorCode::BelowMinimumPurchase.into());
        }
        if user_vesting
            .total_purchased_sol
            .checked_add(total_cost_in_usdc)
            .ok_or(ErrorCode::MathOverflow)?
            > presale_account.max_buy_amount_sol
        {
            return Err(ErrorCode::ExceedsMaximumPurchase.into());
        }
    
        // Ensure the presale hard cap is not exceeded
        if presale_account
            .total_sold_sol_equivalent
            .checked_add(total_cost_in_usdc)
            .ok_or(ErrorCode::MathOverflow)?
            > presale_account.hard_cap_sol
        {
            return Err(ErrorCode::HardCapReached.into());
        }
    
        // Ensure the buyer has enough SOL to pay
        if **ctx.accounts.buyer.to_account_info().lamports.borrow() < total_cost_in_sol {
            return Err(ErrorCode::InsufficientFunds.into());
        }
    
        // Transfer SOL to the program's PDA
        let program_pda = ctx.accounts.presale_account.to_account_info().key;
        **ctx.accounts.buyer.to_account_info().try_borrow_mut_lamports()? -= total_cost_in_sol;
        **program_pda.try_borrow_mut_lamports()? += total_cost_in_sol;
    
        // Update total sales and user metrics
        presale_account.total_sold_in_sol = presale_account
            .total_sold_in_sol
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        user_vesting.total_amount = user_vesting
            .total_amount
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        user_vesting.total_purchased_sol = user_vesting
            .total_purchased_sol
            .checked_add(total_cost_in_usdc)
            .ok_or(ErrorCode::MathOverflow)?;
        presale_account.total_sold_sol_equivalent = presale_account
            .total_sold_sol_equivalent
            .checked_add(total_cost_in_usdc)
            .ok_or(ErrorCode::MathOverflow)?;
    
        // Emit the purchase event
        emit!(PurchaseEvent {
            buyer: ctx.accounts.buyer.key(),
            amount,
            cost_in_sol: total_cost_in_sol,
        });
    
        Ok(())
    }        

    // Utility function to derive the program's PDA
    fn derive_presale_pda(program_id: &Pubkey, seed: &[u8]) -> Pubkey {
        Pubkey::find_program_address(&[seed], program_id).0
    }
    
    // Validation function to ensure oracle feed authenticity
    fn validate_oracle_feed(oracle_feed: &AccountInfo) -> ProgramResult {
        if !oracle_feed.owner.eq(&pyth_client::ID) {
            return Err(ErrorCode::UnauthorizedAccess.into());
        }
        Ok(())
    }
    
    fn get_sol_to_usdc_rate(price_feed: &AccountInfo) -> Result<u64, ProgramError> {
        let price_data = pyth_client::load_price_feed_from_account(price_feed)?;
        let price = price_data.get_current_price().ok_or(ErrorCode::PriceFeedUnavailable)?;
        Ok(price.price as u64)
    }

    // Batch airdrop distribution to save compute units
    pub fn distribute_airdrops_batch(
        ctx: Context<BatchDistributeAirdrops>, 
        users: Vec<UserDistribution>
    ) -> ProgramResult {
        let presale_account = &ctx.accounts.presale_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure presale has ended
        if current_time < presale_account.presale_end {
            return Err(ErrorCode::PresaleNotEnded.into());
        }

        for user in users.iter() {
            let user_vesting = &mut ctx.remaining_accounts[user.user_vesting_index]; // Dynamically load user vesting account
            let airdrop_percentage = *presale_account
                .airdrop_percentages
                .get(user.airdrop_index as usize)
                .ok_or(ErrorCode::AirdropConfigurationError)?;

            // Calculate the airdrop amount
            let airdrop_amount = user_vesting
                .total_amount
                .checked_mul(airdrop_percentage)
                .ok_or(ErrorCode::MathOverflow)?
                .checked_div(100)
                .ok_or(ErrorCode::MathOverflow)?;

            // Transfer the airdrop tokens
            token::transfer(
                ctx.accounts.into_transfer_context(&user_vesting),
                airdrop_amount,
            )?;

            // Update the user vesting account
            user_vesting.claimed_amount += airdrop_amount;
            user_vesting.airdrops_completed += 1;
        }

        Ok(())
    }

    #[derive(AnchorDeserialize, AnchorSerialize, Clone)]
    pub struct UserDistribution {
        pub user_vesting_index: usize, // Index in the remaining accounts array
        pub airdrop_index: u8,         // Index of the current airdrop percentage
    }

    #[event]
    pub struct ClaimEvent {
        pub user: Pubkey,         // User's public key
        pub amount: u64,          // Amount of tokens claimed
        pub total_claimed: u64,   // Total claimed tokens after this transaction
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
            token::transfer(
                ctx.accounts.into_transfer_context(),
                claimable_amount,
            )?;

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

    // Helper to calculate vested amount
    fn calculate_vested_amount(
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
        let vested_intervals = elapsed_time / vesting_interval;
        let total_intervals = vesting_period / vesting_interval;

        // Calculate the proportion of vested tokens
        let vested_amount = (total_amount as u128 * vested_intervals as u128 / total_intervals as u128) as u64;
        vested_amount
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

    #[derive(Accounts)]
    pub struct CalculateClaimable<'info> {
        #[account(mut)]
        pub user_vesting: Account<'info, UserVesting>,
        pub presale_account: Account<'info, PresaleAccount>,
    }

    #[account]
    pub struct PresaleAccount {
        pub presale_start: i64,
        pub presale_end: i64,
        pub public_sale_start: i64,
        pub price: u64,                  // Price in USDC without discount
        pub discount_percentage: u32,   // Discount percentage during presale
        pub max_allocation: u64,
        pub cliff_period: i64,
        pub vesting_period: i64,
        pub vesting_interval: i64,
        pub total_airdrop_periods: u8,
        pub airdrop_percentages: Vec<u8>,
        pub total_sold_in_sol: u64,      // Total sold in SOL
        pub sol_to_usdc_feed: Pubkey,    // Oracle price feed for SOL to USDC
        pub min_buy_amount_sol: u64,     // Minimum SOL amount per purchase
        pub max_buy_amount_sol: u64,     // Maximum SOL amount per user
        pub hard_cap_sol: u64,           // Maximum SOL equivalent for the entire presale
        pub total_sold_sol_equivalent: u64, // Track total SOL-equivalent sales
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
        new_discount_percentage: Option<u64>,
    ) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;
    
        // Validate and update price directly
        if let Some(price) = new_price {
            if price == 0 {
                return Err(ErrorCode::InvalidPrice.into());
            }
            presale_account.price = price;
    
            // Recalculate the discount percentage (based on the original price, if any)
            if presale_account.discount_percentage > 0 {
                let original_price = presale_account.price.checked_mul(100).ok_or(ErrorCode::MathOverflow)?
                    .checked_div(100 - presale_account.discount_percentage).ok_or(ErrorCode::MathOverflow)?;
                presale_account.discount_percentage = (100 - price.checked_mul(100).ok_or(ErrorCode::MathOverflow)?
                    .checked_div(original_price).ok_or(ErrorCode::MathOverflow)?) as u64;
            }
        }
    
        // Validate and update discount percentage
        if let Some(discount) = new_discount_percentage {
            if discount > 100 {
                return Err(ErrorCode::InvalidDiscountPercentage.into());
            }
            presale_account.discount_percentage = discount;
    
            // Recalculate the price based on the discount
            presale_account.price = presale_account.price.checked_mul(100 - discount)
                .ok_or(ErrorCode::MathOverflow)?
                .checked_div(100)
                .ok_or(ErrorCode::MathOverflow)?;
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
        let initial_percentage = *presale_account.airdrop_percentages.get(0).ok_or(ErrorCode::AirdropConfigurationError)?;
        let initial_airdrop = user_vesting
            .total_amount
            .checked_mul(initial_percentage)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(100)
            .ok_or(ErrorCode::MathOverflow)?;

        // Transfer initial airdrop
        token::transfer(
            ctx.accounts.into_transfer_context(),
            initial_airdrop,
        )?;

        // Update claimed amount and airdrop count
        user_vesting.claimed_amount += initial_airdrop;
        user_vesting.airdrops_completed = 1;

        Ok(())
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
        pub presale_account: Account<'info, PresaleAccount>,  // Added
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
        pub token_program: Program<'info, Token>,
        pub system_program: Program<'info, System>,
        /// CHECK: This is the oracle price feed account
        #[account(mut)]
        pub sol_to_usdc_feed: AccountInfo<'info>, // Oracle price feed for SOL to USDC
    }

    pub fn distribute_monthly_airdrop(ctx: Context<DistributeAirdrop>) -> ProgramResult {
        let user_vesting = &mut ctx.accounts.user_vesting;
        let presale_account = &ctx.accounts.presale_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Ensure at least one month has passed since the last airdrop
        let months_elapsed = (current_time - user_vesting.start_time) / presale_account.vesting_interval;
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
        token::transfer(
            ctx.accounts.into_transfer_context(),
            airdrop_amount,
        )?;

        // Update claimed amount and airdrop count
        user_vesting.claimed_amount += airdrop_amount;
        user_vesting.airdrops_completed += 1;

        Ok(())
    }

    #[error]
    pub enum ErrorCode {
        #[msg("The sale is not currently active.")]
        SaleNotActive,
        #[msg("No tokens available for claiming.")]
        NoTokensToClaim,
        #[msg("The presale has not ended.")]
        PresaleNotEnded,
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
    }
}
