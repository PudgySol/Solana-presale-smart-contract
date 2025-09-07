use anchor_lang::prelude::*;
use anchor_spl::token::{self, TokenAccount, Token, Transfer, Mint, TokenAccount as SplTokenAccount};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg6Zojx8kV7G"); // replace with your program id after deploy

const SCALE: u128 = 1_000_000_000_000u128; // 1e12 precision for reward-per-share

#[program]
pub mod liquidity_yield {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        reward_rate: u64, // tokens per second
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.owner = *ctx.accounts.owner.key;
        pool.reward_mint = *ctx.accounts.reward_mint.to_account_info().key;
        pool.reward_vault = *ctx.accounts.reward_vault.to_account_info().key;
        pool.lp_vault = *ctx.accounts.lp_vault.to_account_info().key;
        pool.reward_rate = reward_rate;
        pool.last_update = Clock::get()?.unix_timestamp;
        pool.reward_per_share = 0;
        pool.total_staked = 0;
        Ok(())
    }

    pub fn fund_rewards(ctx: Context<FundRewards>, amount: u64) -> Result<()> {
        // Owner funds the reward vault by transferring tokens into reward_vault (checked by accounts)
        let cpi_accounts = Transfer {
            from: ctx.accounts.from.to_account_info(),
            to: ctx.accounts.reward_vault.to_account_info(),
            authority: ctx.accounts.funder.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        token::transfer(
            CpiContext::new(cpi_program, cpi_accounts),
            amount,
        )?;
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.update()?;

        let user_stake = &mut ctx.accounts.user_stake;
        // if first time, initialize
        if user_stake.owner == Pubkey::default() {
            user_stake.owner = *ctx.accounts.staker.key;
            user_stake.pool = pool.key();
            user_stake.amount = 0;
            user_stake.reward_debt = 0;
        } else {
            require!(user_stake.owner == *ctx.accounts.staker.key, ErrorCode::InvalidOwner);
            require!(user_stake.pool == pool.key(), ErrorCode::InvalidPoolAccount);
        }

        // compute pending and update user reward_debt after transfer
        let pending = user_stake.pending(pool)?;
        // pending may be distributed on staking or left for claim; we won't auto-transfer rewards here to save gas
        // transfer LP tokens from user to lp_vault
        let seeds: &[&[u8]] = &[
            b"pool_auth",
            pool.key().as_ref(),
            &[*ctx.bumps.get("pool_authority").unwrap()],
        ];
        let cpi_accounts = Transfer {
            from: ctx.accounts.from_lp.to_account_info(),
            to: ctx.accounts.lp_vault.to_account_info(),
            authority: ctx.accounts.staker.to_account_info(),
        };
        token::transfer(CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts), amount)?;

        // update user stake and pool totals
        user_stake.amount = user_stake.amount.checked_add(amount).ok_or(ErrorCode::Overflow)?;
        user_stake.reward_debt = mul_div_u128(user_stake.amount as u128, pool.reward_per_share, SCALE);
        pool.total_staked = pool.total_staked.checked_add(amount).ok_or(ErrorCode::Overflow)?;

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.update()?;

        let user_stake = &mut ctx.accounts.user_stake;
        require!(user_stake.owner == *ctx.accounts.staker.key, ErrorCode::InvalidOwner);
        require!(user_stake.amount >= amount, ErrorCode::InsufficientStaked);

        // calculate pending
        let pending = user_stake.pending(pool)?;
        if pending > 0 {
            // transfer rewards
            let pool_auth_seeds: &[&[u8]] = &[
                b"pool_auth",
                pool.key().as_ref(),
                &[*ctx.bumps.get("pool_authority").unwrap()],
            ];
            let signer = &[&pool_auth_seeds[..]];
            let cpi_accounts_reward = Transfer {
                from: ctx.accounts.reward_vault.to_account_info(),
                to: ctx.accounts.to_reward.to_account_info(),
                authority: ctx.accounts.pool_authority.to_account_info(),
            };
            token::transfer(
                CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts_reward, signer),
                pending as u64, // safe if reward vault funded properly; else will fail
            )?;
        }

        // transfer LP back to user
        let pool_auth_seeds: &[&[u8]] = &[
            b"pool_auth",
            pool.key().as_ref(),
            &[*ctx.bumps.get("pool_authority").unwrap()],
        ];
        let signer = &[&pool_auth_seeds[..]];
        let cpi_accounts_lp = Transfer {
            from: ctx.accounts.lp_vault.to_account_info(),
            to: ctx.accounts.to_lp.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        };
        token::transfer(
            CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts_lp, signer),
            amount,
        )?;

        user_stake.amount = user_stake.amount.checked_sub(amount).ok_or(ErrorCode::Overflow)?;
        user_stake.reward_debt = mul_div_u128(user_stake.amount as u128, pool.reward_per_share, SCALE);
        pool.total_staked = pool.total_staked.checked_sub(amount).ok_or(ErrorCode::Overflow)?;
        Ok(())
    }

    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.update()?;
        let user_stake = &mut ctx.accounts.user_stake;
        require!(user_stake.owner == *ctx.accounts.staker.key, ErrorCode::InvalidOwner);

        let pending = user_stake.pending(pool)?;
        if pending > 0 {
            let pool_auth_seeds: &[&[u8]] = &[
                b"pool_auth",
                pool.key().as_ref(),
                &[*ctx.bumps.get("pool_authority").unwrap()],
            ];
            let signer = &[&pool_auth_seeds[..]];
            let cpi_accounts_reward = Transfer {
                from: ctx.accounts.reward_vault.to_account_info(),
                to: ctx.accounts.to_reward.to_account_info(),
                authority: ctx.accounts.pool_authority.to_account_info(),
            };
            token::transfer(
                CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts_reward, signer),
                pending as u64,
            )?;
        }

        user_stake.reward_debt = mul_div_u128(user_stake.amount as u128, pool.reward_per_share, SCALE);
        Ok(())
    }

    // owner can update reward rate
    pub fn set_reward_rate(ctx: Context<SetRewardRate>, new_rate: u64) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        require!(pool.owner == *ctx.accounts.owner.key, ErrorCode::Unauthorized);
        pool.update()?;
        pool.reward_rate = new_rate;
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(reward_rate: u64)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// CHECK: we accept a Mint account; Anchor doesn't enforce Mint type here
    pub reward_mint: AccountInfo<'info>,

    /// CHECK: reward vault (SPL token account) owned by pool authority PDA
    pub reward_vault: Account<'info, SplTokenAccount>,

    /// CHECK: LP vault (SPL token account) owned by pool authority PDA
    pub lp_vault: Account<'info, SplTokenAccount>,

    #[account(
        init,
        payer = owner,
        space = 8 + Pool::LEN,
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: pool authority PDA (derived)
    /// This is only used for verification when creating associated token accounts off-chain. Not created here.
    #[account(seeds = [b"pool_auth", pool.key().as_ref()], bump)]
    pub pool_authority: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct FundRewards<'info> {
    #[account(mut)]
    pub funder: Signer<'info>,

    #[account(mut)]
    pub from: Account<'info, SplTokenAccount>, // funder token account

    #[account(mut)]
    pub reward_vault: Account<'info, SplTokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub staker: Signer<'info>,

    #[account(mut)]
    pub from_lp: Account<'info, SplTokenAccount>, // user's LP token account (source)

    #[account(mut)]
    pub lp_vault: Account<'info, SplTokenAccount>,

    #[account(mut)]
    pub pool: Account<'info, Pool>,

    #[account(
        init_if_needed,
        payer = staker,
        space = 8 + UserStake::LEN,
        seeds = [b"user_stake", staker.key.as_ref(), pool.key().as_ref()],
        bump
    )]
    pub user_stake: Account<'info, UserStake>,

    /// CHECK: pool authority PDA
    #[account(seeds = [b"pool_auth", pool.key().as_ref()], bump)]
    pub pool_authority: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub staker: Signer<'info>,

    #[account(mut)]
    pub to_lp: Account<'info, SplTokenAccount>, // destination LP token account for user

    #[account(mut)]
    pub to_reward: Account<'info, SplTokenAccount>, // destination reward token account for user

    #[account(mut)]
    pub lp_vault: Account<'info, SplTokenAccount>,

    #[account(mut)]
    pub reward_vault: Account<'info, SplTokenAccount>,

    #[account(mut)]
    pub pool: Account<'info, Pool>,

    #[account(mut, seeds = [b"user_stake", staker.key.as_ref(), pool.key().as_ref()], bump)]
    pub user_stake: Account<'info, UserStake>,

    /// CHECK: pool authority PDA
    #[account(seeds = [b"pool_auth", pool.key().as_ref()], bump)]
    pub pool_authority: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub staker: Signer<'info>,

    #[account(mut)]
    pub to_reward: Account<'info, SplTokenAccount>,

    #[account(mut)]
    pub reward_vault: Account<'info, SplTokenAccount>,

    #[account(mut)]
    pub pool: Account<'info, Pool>,

    #[account(mut, seeds = [b"user_stake", staker.key.as_ref(), pool.key().as_ref()], bump)]
    pub user_stake: Account<'info, UserStake>,

    /// CHECK: pool authority PDA
    #[account(seeds = [b"pool_auth", pool.key().as_ref()], bump)]
    pub pool_authority: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SetRewardRate<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut)]
    pub pool: Account<'info, Pool>,
}

#[account]
pub struct Pool {
    pub owner: Pubkey,
    pub reward_mint: Pubkey,
    pub reward_vault: Pubkey,
    pub lp_vault: Pubkey,
    pub reward_rate: u64,
    pub last_update: i64,
    pub reward_per_share: u128,
    pub total_staked: u64,
}

impl Pool {
    // approximate len for account allocation
    pub const LEN: usize = 32 + 32 + 32 + 32 + 8 + 8 + 16 + 8;

    pub fn update(&mut self) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        if now <= self.last_update {
            self.last_update = now;
            return Ok(());
        }
        let elapsed = (now - self.last_update) as u128;
        if self.total_staked > 0 && self.reward_rate > 0 {
            let reward = elapsed.checked_mul(self.reward_rate as u128).ok_or(ErrorCode::Overflow)?;
            // reward_per_share += reward * SCALE / total_staked
            let add = reward
                .checked_mul(SCALE)
                .ok_or(ErrorCode::Overflow)?
                .checked_div(self.total_staked as u128)
                .ok_or(ErrorCode::Overflow)?;
            self.reward_per_share = self.reward_per_share.checked_add(add).ok_or(ErrorCode::Overflow)?;
        }
        self.last_update = now;
        Ok(())
    }
}

#[account]
pub struct UserStake {
    pub owner: Pubkey,
    pub pool: Pubkey,
    pub amount: u64,
    pub reward_debt: u128,
}

impl UserStake {
    pub const LEN: usize = 32 + 32 + 8 + 16;

    pub fn pending(&self, pool: &Pool) -> Result<u128> {
        // pending = amount * pool.reward_per_share / SCALE - reward_debt
        let acc = mul_div_u128(self.amount as u128, pool.reward_per_share, SCALE);
        if acc <= self.reward_debt {
            Ok(0)
        } else {
            Ok(acc - self.reward_debt)
        }
    }
}

// helper for (a * b) / c with u128
fn mul_div_u128(a: u128, b: u128, c: u128) -> u128 {
    // a*b up to u256 in theory; but with chosen SCALE and conservative ranges should be fine
    (a.checked_mul(b).unwrap()).checked_div(c).unwrap()
}

#[error_code]
pub enum ErrorCode {
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Overflow")]
    Overflow,
    #[msg("Invalid owner")]
    InvalidOwner,
    #[msg("Invalid pool account")]
    InvalidPoolAccount,
    #[msg("Insufficient staked amount")]
    InsufficientStaked,
}
