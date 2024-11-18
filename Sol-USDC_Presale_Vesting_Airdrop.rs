use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("YourProgramIdHere");

#[program]
pub mod main_presale_contract {
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
    ) -> ProgramResult {
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
    
        Ok(())
    }    

    pub struct UserVesting {
    pub total_amount: u64,           // Total tokens purchased
    pub claimed_amount: u64,         // Tokens already claimed
    pub start_time: i64,             // Presale end time
    pub airdrops_completed: u8,      // Number of airdrops already distributed
}

    // Purchase tokens with vesting
    pub fn purchase(ctx: Context<Purchase>, amount: u64, payment_method: PaymentMethod) -> ProgramResult {
        let presale_account = &mut ctx.accounts.presale_account;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
    
        // Ensure the sale is active
        if !(current_time >= presale_account.presale_start && current_time <= presale_account.presale_end) {
            return Err(ErrorCode::SaleNotActive.into());
        }
    
        // Calculate total cost in USDC
        let total_cost_in_usdc = amount.checked_mul(presale_account.price).ok_or(ErrorCode::MathOverflow)?;
    
        match payment_method {
            PaymentMethod::SOL => {
                // Fetch the SOL to USDC conversion rate from the oracle
                let sol_to_usdc_rate = get_sol_to_usdc_rate(&ctx.accounts.sol_to_usdc_feed)?;
                
                // Convert USDC cost to SOL
                let required_sol = total_cost_in_usdc
                    .checked_mul(10u64.pow(9)) // Scale to lamports
                    .ok_or(ErrorCode::MathOverflow)?
                    .checked_div(sol_to_usdc_rate)
                    .ok_or(ErrorCode::MathOverflow)?;
    
                // Ensure buyer sent enough SOL
                if **ctx.accounts.buyer.to_account_info().lamports.borrow() < required_sol {
                    return Err(ErrorCode::InsufficientFunds.into());
                }
    
                // Transfer SOL to program
                **ctx.accounts.buyer.to_account_info().try_borrow_mut_lamports()? -= required_sol;
                **ctx.accounts.program.to_account_info().try_borrow_mut_lamports()? += required_sol;
    
                // Update total sold in SOL
                presale_account.total_sold_in_sol = presale_account.total_sold_in_sol.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;
            }
            PaymentMethod::USDC => {
                // Ensure buyer has enough USDC
                let buyer_balance = ctx.accounts.buyer_token_account.amount;
                if buyer_balance < total_cost_in_usdc {
                    return Err(ErrorCode::InsufficientFunds.into());
                }
    
                // Transfer USDC to program's token account
                token::transfer(
                    ctx.accounts.into_transfer_to_program_context(),
                    total_cost_in_usdc,
                )?;
    
                // Update total sold in USDC
                presale_account.total_sold_in_usdc = presale_account.total_sold_in_usdc.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;
            }
        }
    
        // Record vesting information
        let user_vesting = &mut ctx.accounts.user_vesting;
        user_vesting.total_amount = amount;
        user_vesting.claimed_amount = 0;
        user_vesting.start_time = current_time + presale_account.cliff_period;
    
        Ok(())
    }
    
    fn get_sol_to_usdc_rate(price_feed: &AccountInfo) -> Result<u64, ProgramError> {
        let price_data = pyth_client::load_price_feed_from_account(price_feed)?;
        let price = price_data.get_current_price().ok_or(ErrorCode::PriceFeedUnavailable)?;
        Ok(price.price as u64)
    }

    // Claim vested tokens
    pub fn claim(ctx: Context<Claim>) -> ProgramResult {
        let user_vesting = &mut ctx.accounts.user_vesting;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Calculate vested tokens
        let vested_amount = calculate_vested_amount(
            user_vesting.total_amount,
            user_vesting.start_time,
            presale_account.vesting_period,
            presale_account.vesting_interval,
            current_time,
        );

        // Check claimable amount
        let claimable_amount = vested_amount.saturating_sub(user_vesting.claimed_amount);
        if claimable_amount > 0 {
            // Transfer vested tokens
            token::transfer(
                ctx.accounts.into_transfer_context(),
                claimable_amount,
            )?;

            // Update claimed amount
            user_vesting.claimed_amount += claimable_amount;
        } else {
            return Err(ErrorCode::NoTokensToClaim.into());
        }

        Ok(())
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
pub struct UserVesting {
    pub total_amount: u64,           // Total tokens purchased
    pub claimed_amount: u64,         // Tokens already claimed
    pub start_time: i64,             // Presale end time
    pub airdrops_completed: u8,      // Number of airdrops already distributed
}

#[account]
pub struct PresaleAccount {
    pub presale_start: i64,
    pub presale_end: i64,
    pub public_sale_start: i64,
    pub price: u64,                // Price in USDC
    pub max_allocation: u64,
    pub cliff_period: i64,
    pub vesting_period: i64,
    pub vesting_interval: i64,
    pub total_sold_in_sol: u64,    // Track sales in SOL
    pub total_sold_in_usdc: u64,   // Track sales in USDC
    pub sol_to_usdc_feed: Pubkey,  // Oracle price feed account for SOL to USDC
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

pub fn distribute_initial_airdrop(ctx: Context<DistributeAirdrop>) -> ProgramResult {
    let user_vesting = &mut ctx.accounts.user_vesting;
    let clock = Clock::get()?;
    let current_time = clock.unix_timestamp;

    // Ensure presale has ended
    if current_time < user_vesting.start_time {
        return Err(ErrorCode::PresaleNotEnded.into());
    }

    // Distribute 10% of the total purchase amount
    let initial_airdrop = user_vesting.total_amount / 10;
    token::transfer(
        ctx.accounts.into_transfer_context(),
        initial_airdrop,
    )?;

    // Update claimed amount and airdrop count
    user_vesting.claimed_amount += initial_airdrop;
    user_vesting.airdrops_completed = 1;

    Ok(())
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
    #[account(mut)]
    pub buyer_token_account: Account<'info, TokenAccount>, // USDC token account of buyer
    #[account(mut)]
    pub program_token_account: Account<'info, TokenAccount>, // USDC token account of program
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    /// CHECK: This is the oracle price feed account
    #[account(mut)]
    pub sol_to_usdc_feed: AccountInfo<'info>, // Oracle price feed for SOL to USDC
}

pub fn distribute_monthly_airdrop(ctx: Context<DistributeAirdrop>) -> ProgramResult {
    let user_vesting = &mut ctx.accounts.user_vesting;
    let clock = Clock::get()?;
    let current_time = clock.unix_timestamp;

    // Ensure at least one month has passed since the last airdrop
    let months_elapsed = (current_time - user_vesting.start_time) / (30 * 24 * 60 * 60);
    if months_elapsed as u8 <= user_vesting.airdrops_completed {
        return Err(ErrorCode::AirdropNotDue.into());
    }
    if user_vesting.airdrops_completed >= 10 {
        return Err(ErrorCode::AirdropCompleted.into());
    }

    // Calculate the next 10% to distribute
    let next_airdrop = user_vesting.total_amount / 10;
    token::transfer(
        ctx.accounts.into_transfer_context(),
        next_airdrop,
    )?;

    // Update claimed amount and airdrop count
    user_vesting.claimed_amount += next_airdrop;
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
}
