use anchor_lang::prelude::*;
use anchor_lang::system_program;
#[allow(unused_imports)]
use pyth_sdk_solana::load_price_feed_from_account_info;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use solana_program::{
    account_info::AccountInfo,
    pubkey::Pubkey,
    clock::Clock,
};

declare_id!("Your Program ID");

#[program]
pub mod presale_vesting {
    use super::*;

    pub fn initialize_presale(
        ctx: Context<InitializePresale>,
        token_mint: Pubkey,
        admin: Pubkey,
        bump: u8,
        public_sale_price: u64,
        max_tokens: u64,
        max_sol: u64, // Add SOL hard cap
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;
        presale.token_mint = token_mint;
        presale.admin = admin;
        presale.total_tokens_allocated = 0;
        presale.max_tokens = max_tokens;
        presale.total_sol_collected = 0; // Initialize total SOL collected
        presale.max_sol = max_sol; // Set the SOL hard cap
        presale.is_closed = false;
        presale.bump = bump; // Save the bump seed
        presale.public_sale_price = public_sale_price; // Set the public sale price
        Ok(())
    }

    pub fn contribute(ctx: Context<Contribute>, lamports_paid: u64) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;

        // Ensure the presale has not ended
        require!(!presale.is_closed, CustomError::PresaleClosed);

        // Calculate the discounted price (85% of public sale price)
        let discounted_price = presale.public_sale_price * 85 / 100; // 15% discount

        // Calculate the number of tokens the contributor receives
        let tokens_to_allocate = lamports_paid / discounted_price; // Tokens = lamports paid / discounted price
        require!(tokens_to_allocate > 0, CustomError::InvalidContribution);

        // Ensure the total SOL collected does not exceed the cap
        let remaining_sol_cap = presale.max_sol - presale.total_sol_collected;
        require!(lamports_paid <= remaining_sol_cap, CustomError::PresaleLimitReached);

        // Transfer only the portion of lamports that fits within the SOL cap
        let lamports_to_accept = lamports_paid.min(remaining_sol_cap);
        **ctx.accounts.admin_wallet.to_account_info().try_borrow_mut_lamports()? += lamports_to_accept;
        **ctx.accounts.contributor.to_account_info().try_borrow_mut_lamports()? -= lamports_to_accept;

        // Update the total SOL collected
        presale.total_sol_collected += lamports_to_accept;

        // Update allocation account with the contributed amount
        let allocation = &mut ctx.accounts.allocation_account;
        allocation.amount += lamports_to_accept / discounted_price; // Allocate tokens
        allocation.cliff_timestamp = presale.cliff_timestamp;
        allocation.vesting_end_timestamp = presale.vesting_end_timestamp;

        presale.total_tokens_allocated += lamports_to_accept / discounted_price;

        Ok(())
    }

    pub fn claim_tokens(ctx: Context<ClaimTokens>, claimable_now: u64) -> Result<()> {
        let allocation = &mut ctx.accounts.allocation_account;

        // Create a longer-lived variable for the presale_account key
        let presale_account_key = ctx.accounts.presale_account.key();

        // Prepare seeds for signing
        let seeds = &[
            b"presale",
            presale_account_key.as_ref(),
            &[ctx.accounts.presale_account.bump],
        ];
        let signer = &[&seeds[..]];

        // Transfer tokens to the contributor
        let cpi_accounts = Transfer {
            from: ctx.accounts.presale_wallet.to_account_info(), // Token source
            to: ctx.accounts.contributor_wallet.to_account_info(), // Token destination
            authority: ctx.accounts.presale_account.to_account_info(), // Authority
        };
        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer,
        );
        token::transfer(cpi_context, claimable_now)?;

        // Update allocation
        allocation.claimed_amount += claimable_now;

        Ok(())
    }

    /// New Airdrop Function for Automated Token Vesting
    pub fn airdrop_tokens(ctx: Context<AirdropTokens>) -> Result<()> {
        let allocation = &mut ctx.accounts.allocation_account;
        let presale = &ctx.accounts.presale_account;

        // Ensure the presale has ended
        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(current_time >= allocation.cliff_timestamp, CustomError::CliffNotReached);

        // Calculate claimable tokens based on the vesting schedule
        let total_allocation = allocation.amount;
        let upfront_allocation = total_allocation / 10; // 10% upfront
        let monthly_allocation = total_allocation * 9 / 100; // 9% monthly

        let months_elapsed = ((current_time - allocation.cliff_timestamp) / (30 * 24 * 60 * 60)) as u64;

        let mut claimable_amount = 0;

        if months_elapsed == 0 {
            // If we're at the end of the cliff, release 10% upfront
            claimable_amount = upfront_allocation;
        } else if months_elapsed > 0 {
            // Calculate the total claimable amount based on elapsed months
            let max_months = 10; // 10 months of vesting
            let months_to_claim = months_elapsed.min(max_months);
            claimable_amount = upfront_allocation + (monthly_allocation * months_to_claim);
        }

        // Ensure we do not over-distribute tokens
        claimable_amount = claimable_amount.saturating_sub(allocation.claimed_amount);

        require!(claimable_amount > 0, CustomError::NothingToClaim);

        // Prepare seeds for signing
        let presale_key = presale.key();
        let seeds = &[
            b"presale",
            presale_key.as_ref(),
            &[presale.bump],
        ];
        let signer = &[&seeds[..]];

        // Transfer claimable tokens to the contributor
        let cpi_accounts = Transfer {
            from: ctx.accounts.presale_wallet.to_account_info(), // Token source
            to: ctx.accounts.contributor_wallet.to_account_info(), // Token destination
            authority: ctx.accounts.presale_account.to_account_info(), // Authority
        };
        let cpi_context = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer,
        );
        token::transfer(cpi_context, claimable_amount)?;

        // Update allocation to reflect claimed tokens
        allocation.claimed_amount += claimable_amount;

        Ok(())
    }

    pub fn update_presale_price(ctx: Context<UpdatePresalePrice>, new_public_sale_price: u64) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;

        // Ensure the presale is still active
        require!(!presale.is_closed, CustomError::PresaleClosed);

        // Ensure only the admin can update the price
        require!(ctx.accounts.admin.key() == presale.admin, CustomError::Unauthorized);

        // Update the public sale price
        presale.public_sale_price = new_public_sale_price;

        Ok(())
    }


    pub fn refund_tokens(ctx: Context<RefundTokens>, token_amount: u64) -> Result<()> {
        let allocation = &mut ctx.accounts.allocation_account;
        let presale = &mut ctx.accounts.presale_account;

        // Ensure the presale is closed or refund is allowed
        require!(presale.is_closed, CustomError::PresaleClosed);

        // Ensure the refund is requested before vesting begins
        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(current_time < allocation.cliff_timestamp, CustomError::VestingStarted);

        // Ensure the contributor has enough tokens to refund
        require!(allocation.amount >= token_amount, CustomError::InsufficientBalance);

        // Calculate the equivalent refund in lamports
        let discounted_price = presale.public_sale_price * 85 / 100; // 15% discount
        let lamports_to_refund = token_amount * discounted_price;

        // Perform the refund (lamports transfer)
        **ctx.accounts.contributor.to_account_info().try_borrow_mut_lamports()? += lamports_to_refund;
        **ctx.accounts.presale_wallet.to_account_info().try_borrow_mut_lamports()? -= lamports_to_refund;

        // Burn or transfer tokens from the contributor back to the presale wallet
        let cpi_accounts = Transfer {
            from: ctx.accounts.contributor_wallet.to_account_info(), // Token source
            to: ctx.accounts.presale_wallet.to_account_info(),       // Token destination
            authority: ctx.accounts.contributor.to_account_info(),   // Contributor's authority
        };
        let cpi_context = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_context, token_amount)?;

        // Update the allocation
        allocation.amount -= token_amount;

        Ok(())
    }

    pub fn close_presale(ctx: Context<ClosePresale>) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;

        // Only the admin can close the presale
        require!(ctx.accounts.admin.key() == presale.admin, CustomError::Unauthorized);
        presale.is_closed = true;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePresale<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + 32 + 32 + 8 + 8 + 8 + 8 + 1 // Add 8 bytes for the max_tokens field
    )]
    pub presale_account: Account<'info, PresaleAccount>,
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdatePresalePrice<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale state
    #[account(signer)]
    pub admin: AccountInfo<'info>, // Admin must sign the transaction
}

#[derive(Accounts)]
pub struct Contribute<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale state
    #[account(
        init_if_needed,
        payer = contributor,
        space = 8 + 8 + 8 + 8 + 8
    )]
    pub allocation_account: Account<'info, AllocationAccount>, // Allocation state for the contributor
    #[account(mut)]
    pub contributor: Signer<'info>, // Contributor wallet
    /// CHECK: Admin wallet account (could add stricter validation here)
    pub admin_wallet: AccountInfo<'info>, // Admin wallet to receive funds
    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>, // Token program
    pub system_program: Program<'info, System>, // System program
}

#[derive(Accounts)]
pub struct ClaimTokens<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale state
    #[account(mut)]
    pub allocation_account: Account<'info, AllocationAccount>, // Allocation state for the contributor
    #[account(mut)]
    pub presale_wallet: Account<'info, TokenAccount>, // Presale token wallet
    #[account(mut)]
    pub contributor_wallet: Account<'info, TokenAccount>, // Contributor token wallet
    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>, // Token program
}

#[derive(Accounts)]
pub struct AirdropTokens<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale state
    #[account(mut)]
    pub allocation_account: Account<'info, AllocationAccount>, // Contributor's allocation
    #[account(mut)]
    pub presale_wallet: Account<'info, TokenAccount>, // Presale token wallet
    #[account(mut)]
    pub contributor_wallet: Account<'info, TokenAccount>, // Contributor token wallet
    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>, // Token program
}

#[derive(Accounts)]
pub struct RefundTokens<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale state
    #[account(mut)]
    pub allocation_account: Account<'info, AllocationAccount>, // Contributor's allocation
    #[account(mut)]
    pub presale_wallet: Account<'info, TokenAccount>, // Presale token wallet
    #[account(mut)]
    pub contributor_wallet: Account<'info, TokenAccount>, // Contributor token wallet
    #[account(mut)]
    pub contributor: Signer<'info>, // Contributor wallet
    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>, // Token program
}

#[derive(Accounts)]
pub struct ClosePresale<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>,
    #[account(signer)]
    pub admin: AccountInfo<'info>,
}

#[account]
pub struct PresaleAccount {
    pub token_mint: Pubkey,              // Token mint address
    pub admin: Pubkey,                  // Admin address
    pub total_tokens_allocated: u64,    // Total tokens allocated so far
    pub max_tokens: u64,                // Maximum tokens allowed in the presale
    pub total_sol_collected: u64,       // Total SOL collected so far (new field)
    pub max_sol: u64,                   // Maximum SOL allowed to be collected (new field)
    pub cliff_timestamp: u64,           // Cliff timestamp for vesting
    pub vesting_end_timestamp: u64,     // Vesting end timestamp
    pub is_closed: bool,                // Whether the presale is closed
    pub bump: u8,                       // PDA bump seed
    pub public_sale_price: u64,         // Token price in public sale (e.g., 1 token = X lamports)
}

#[account]
pub struct AllocationAccount {
    pub amount: u64,
    pub claimed_amount: u64,
    pub cliff_timestamp: u64,
    pub vesting_end_timestamp: u64,
}

#[error_code]
pub enum CustomError {
    #[msg("The presale has already been closed.")]
    PresaleClosed,
    #[msg("The presale has not started yet.")]
    PresaleNotStarted,
    #[msg("The cliff period has not been reached yet.")]
    CliffNotReached,
    #[msg("You have no tokens to claim at this time.")]
    NothingToClaim,
    #[msg("Unauthorized action.")]
    Unauthorized,
    #[msg("Vesting has already started. Refunds are no longer allowed.")]
    VestingStarted,
    #[msg("You do not have enough tokens to refund.")]
    InsufficientBalance,
    #[msg("Invalid contribution. You must contribute enough to purchase at least one token.")]
    InvalidContribution,
    #[msg("The presale SOL limit has been reached.")]
    PresaleLimitReached,
}
