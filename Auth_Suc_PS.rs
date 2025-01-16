use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint};
use anchor_spl::associated_token::AssociatedToken;
#[allow(unused_imports)]
use pyth_sdk_solana::load_price_feed_from_account_info;
use solana_program::{account_info::AccountInfo, clock::Clock, pubkey::Pubkey};

declare_id!("CONTRACT");

#[program]
pub mod presale_vesting {
    use super::*;

    pub fn initialize_presale(
        ctx: Context<InitializePresale>,
        token_mint: Pubkey, // Pass the existing token mint
        admin: Pubkey,
        bump: u8,
        public_sale_price: u64,
        max_tokens: u64,
        max_sol: u64,
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;
        presale.token_mint = token_mint; // Store the token mint address
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
        require!(
            lamports_paid <= remaining_sol_cap,
            CustomError::PresaleLimitReached
        );

        // Transfer only the portion of lamports that fits within the SOL cap
        let lamports_to_accept = lamports_paid.min(remaining_sol_cap);
        **ctx
            .accounts
            .admin_wallet
            .to_account_info()
            .try_borrow_mut_lamports()? += lamports_to_accept;
        **ctx
            .accounts
            .contributor
            .to_account_info()
            .try_borrow_mut_lamports()? -= lamports_to_accept;

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

        // Ensure enough tokens are available for distribution
        let authority_wallet_balance = ctx.accounts.authority_wallet.amount;
        require!(authority_wallet_balance >= claimable_now, CustomError::InsufficientBalance);

        // Transfer tokens to the contributor
        let cpi_accounts = Transfer {
            from: ctx.accounts.authority_wallet.to_account_info(), // Authority wallet's token account
            to: ctx.accounts.contributor_wallet.to_account_info(), // Contributor's token account
            authority: ctx.accounts.admin.to_account_info(), // Admin must sign the transaction
        };
        let cpi_context = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_context, claimable_now)?;

        // Update allocation
        allocation.claimed_amount += claimable_now;

        Ok(())
    }

    /// New Airdrop Function for Automated Token Vesting
    pub fn airdrop_tokens(ctx: Context<AirdropTokens>) -> Result<()> {
        let allocation = &mut ctx.accounts.allocation_account;

        // Ensure the cliff period has been reached
        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(
            current_time >= allocation.cliff_timestamp,
            CustomError::CliffNotReached
        );

        // Calculate claimable tokens
        let total_allocation = allocation.amount;
        let upfront_allocation = total_allocation / 10; // 10% upfront
        let monthly_allocation = total_allocation * 9 / 100; // 9% per month

        // Calculate elapsed months since cliff
        let elapsed_time = current_time.saturating_sub(allocation.cliff_timestamp);
        let months_elapsed = elapsed_time / (30 * 24 * 60 * 60); // 1 month = 30 days

        let max_months = 10; // Total vesting period is 10 months
        let months_to_claim = months_elapsed.min(max_months); // Cap the months to 10

        // Total claimable amount = 10% upfront + (9% * months_elapsed)
        let mut claimable_amount = upfront_allocation + (monthly_allocation * months_to_claim);

        // Deduct already claimed tokens
        claimable_amount = claimable_amount.saturating_sub(allocation.claimed_amount);

        // Ensure there are claimable tokens
        require!(claimable_amount > 0, CustomError::NothingToClaim);

        // Ensure the authority wallet has enough tokens
        let authority_wallet_balance = ctx.accounts.authority_wallet.amount;
        require!(
            authority_wallet_balance >= claimable_amount,
            CustomError::InsufficientBalance
        );

        // Transfer tokens from the authority wallet to the contributor's wallet
        let cpi_accounts = Transfer {
            from: ctx.accounts.authority_wallet.to_account_info(), // Authority wallet's token account
            to: ctx.accounts.contributor_wallet.to_account_info(), // Contributor's token account
            authority: ctx.accounts.admin.to_account_info(), // Admin signs the transaction
        };
        let cpi_context = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_context, claimable_amount)?;

        // Update allocation to reflect the claimed amount
        allocation.claimed_amount += claimable_amount;

        Ok(())
    }

    pub fn update_presale_price(
        ctx: Context<UpdatePresalePrice>,
        new_public_sale_price: u64,
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;

        // Ensure the presale is still active
        require!(!presale.is_closed, CustomError::PresaleClosed);

        // Ensure only the admin can update the price
        require!(
            ctx.accounts.admin.key() == presale.admin,
            CustomError::Unauthorized
        );

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
        require!(
            current_time < allocation.cliff_timestamp,
            CustomError::VestingStarted
        );

        // Ensure the contributor has enough tokens to refund
        require!(
            allocation.amount >= token_amount,
            CustomError::InsufficientBalance
        );

        // Calculate the equivalent refund in lamports
        let discounted_price = presale.public_sale_price * 85 / 100; // 15% discount
        let lamports_to_refund = token_amount * discounted_price;

        // Perform the refund (lamports transfer)
        **ctx
            .accounts
            .contributor
            .to_account_info()
            .try_borrow_mut_lamports()? += lamports_to_refund;
        **ctx
            .accounts
            .presale_wallet
            .to_account_info()
            .try_borrow_mut_lamports()? -= lamports_to_refund;

        // Burn or transfer tokens from the contributor back to the presale wallet
        let cpi_accounts = Transfer {
            from: ctx.accounts.contributor_wallet.to_account_info(), // Token source
            to: ctx.accounts.presale_wallet.to_account_info(),       // Token destination
            authority: ctx.accounts.contributor.to_account_info(),   // Contributor's authority
        };
        let cpi_context =
            CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_context, token_amount)?;

        // Update the allocation
        allocation.amount -= token_amount;

        Ok(())
    }

    pub fn close_presale(ctx: Context<ClosePresale>) -> Result<()> {
        let presale = &mut ctx.accounts.presale_account;

        // Only the admin can close the presale
        require!(
            ctx.accounts.admin.key() == presale.admin,
            CustomError::Unauthorized
        );
        presale.is_closed = true;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePresale<'info> {
    #[account(init, payer = admin, space = 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 1)]
    pub presale_account: Account<'info, PresaleAccount>,
    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = token_mint,
        associated_token::authority = presale_account, // PDA owns the token account
    )]
    pub presale_wallet: Account<'info, TokenAccount>,
    pub token_mint: Account<'info, Mint>,
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(address = system_program::ID)]
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
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
    pub allocation_account: Account<'info, AllocationAccount>, // Contributor's allocation
    #[account(mut)]
    pub authority_wallet: Account<'info, TokenAccount>, // Authority wallet's token account
    #[account(mut)]
    pub contributor_wallet: Account<'info, TokenAccount>, // Contributor's token account
    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>, // Token program
    pub admin: Signer<'info>, // Admin must sign for distribution
}

#[derive(Accounts)]
pub struct AirdropTokens<'info> {
    #[account(mut)]
    pub presale_account: Account<'info, PresaleAccount>, // Presale state
    #[account(mut)]
    pub allocation_account: Account<'info, AllocationAccount>, // Contributor's allocation
    #[account(mut)]
    pub authority_wallet: Account<'info, TokenAccount>, // Authority wallet's token account
    #[account(mut)]
    pub contributor_wallet: Account<'info, TokenAccount>, // Contributor's token wallet
    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>, // Token program
    pub admin: Signer<'info>, // Admin must sign the transaction
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
    pub token_mint: Pubkey,          // Token mint address
    pub admin: Pubkey,               // Admin address
    pub total_tokens_allocated: u64, // Total tokens allocated so far
    pub max_tokens: u64,             // Maximum tokens allowed in the presale
    pub total_sol_collected: u64,    // Total SOL collected so far (new field)
    pub max_sol: u64,                // Maximum SOL allowed to be collected (new field)
    pub cliff_timestamp: u64,        // Cliff timestamp for vesting
    pub vesting_end_timestamp: u64,  // Vesting end timestamp
    pub is_closed: bool,             // Whether the presale is closed
    pub bump: u8,                    // PDA bump seed
    pub public_sale_price: u64,      // Token price in public sale (e.g., 1 token = X lamports)
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
