use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, MintTo, Transfer};

declare_id!("YOUR_PROGRAM_ID");

#[program]
pub mod solana_token_ico {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, token_amount: u64) -> ProgramResult {
        let mint = &ctx.accounts.mint;
        let authority = &ctx.accounts.authority;
        let token_program = &ctx.accounts.token_program;

        let cpi_accounts = MintTo {
            mint: mint.to_account_info().clone(),
            to: ctx.accounts.token_account.to_account_info().clone(),
            authority: authority.to_account_info().clone(),
        };
        let cpi_context = CpiContext::new(token_program.to_account_info(), cpi_accounts);

        token::mint_to(cpi_context, token_amount)?;

        Ok(())
    }

    pub fn buy_tokens(ctx: Context<BuyTokens>, amount: u64) -> ProgramResult {
        let buyer = &ctx.accounts.buyer;
        let token_program = &ctx.accounts.token_program;
        let source_account = &ctx.accounts.source_account;
        let destination_account = &ctx.accounts.destination_account;

        let cpi_accounts = Transfer {
            from: source_account.to_account_info().clone(),
            to: destination_account.to_account_info().clone(),
            authority: buyer.to_account_info().clone(),
        };
        let cpi_context = CpiContext::new(token_program.to_account_info(), cpi_accounts);

        token::transfer(cpi_context, amount)?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = authority, mint::decimals = 6, mint::authority = authority)]
    pub mint: Account<'info, Mint>,
    #[account(init, payer = authority, token::mint = mint, token::authority = authority)]
    pub token_account: Account<'info, TokenAccount>,
    #[account(signer)]
    pub authority: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct BuyTokens<'info> {
    #[account(signer)]
    pub buyer: AccountInfo<'info>,
    #[account(mut)]
    pub source_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub destination_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}
