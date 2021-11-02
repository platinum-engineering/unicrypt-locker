use anchor_lang::{prelude::*, solana_program::clock::UnixTimestamp};
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{self, Token, TokenAccount, Transfer},
};

use az::CheckedAs;
use borsh::{BorshDeserialize, BorshSerialize};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

mod fee {
    use super::*;

    declare_id!("22222222222222222222222222222222222222222222");

    pub const FEE: u64 = 35;
}

#[error]
pub enum ErrorCode {
    #[msg("The given unlock date is in the past")]
    UnlockInThePast,
    #[msg("The given fee wallet is not associated with required fee wallet")]
    InvalidFeeWallet,
    IntegerOverflow,
    NothingToLock,
    InvalidAmountTransferred,
    InvalidPeriod,
}

#[program]
pub mod locker {
    use super::*;

    pub fn create_lock(ctx: Context<CreateLocker>, args: CreateLockerArgs) -> Result<()> {
        let locker = &mut ctx.accounts.locker;

        let now = ctx.accounts.clock.unix_timestamp;
        require!(args.unlock_date > now, UnlockInThePast);

        locker.original_unlock_date = args.unlock_date;
        locker.current_unlock_date = args.unlock_date;

        locker.country_code = args.country_code;

        if let Some(linear_emission) = args.linear_emission.as_ref() {
            require!(
                linear_emission.emission_end > linear_emission.emission_start,
                InvalidPeriod
            );
        }
        locker.linear_emission = args.linear_emission;

        locker.owner = ctx.accounts.owner.key();
        locker.creator = ctx.accounts.creator.key();

        let associated_token_account =
            get_associated_token_address(&fee::ID, &ctx.accounts.funding_wallet.mint);

        require!(
            associated_token_account == ctx.accounts.fee_wallet.key(),
            InvalidFeeWallet
        );

        use fixed::types::U64F64;

        let amount = U64F64::from_num(args.amount);
        let fee = U64F64::from_num(fee::FEE);
        let denominator = U64F64::from_num(10000);

        let amount_before = ctx.accounts.funding_wallet.amount;

        // floor(amount * fee / 10000)
        let lock_fee = amount
            .checked_mul(fee)
            .and_then(|r| r.checked_div(denominator))
            .and_then(|r| r.floor().checked_as::<u64>())
            .ok_or(ErrorCode::IntegerOverflow)?;

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.funding_wallet.to_account_info(),
                to: ctx.accounts.fee_wallet.to_account_info(),
                authority: ctx.accounts.funding_wallet_authority.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, lock_fee)?;

        ctx.accounts.funding_wallet.reload()?;
        let amount_after_fee = ctx.accounts.funding_wallet.amount;
        require!(
            amount_after_fee - amount_before == lock_fee,
            InvalidAmountTransferred
        );

        let amount_to_lock = args.amount - lock_fee;
        require!(amount_to_lock > 0, NothingToLock);

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.funding_wallet.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.funding_wallet_authority.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, amount_to_lock)?;

        ctx.accounts.funding_wallet.reload()?;
        let amount_final = ctx.accounts.funding_wallet.amount;
        require!(
            amount_final - amount_after_fee == amount_to_lock,
            InvalidAmountTransferred
        );

        Ok(())
    }
}

#[account]
pub struct Locker {
    owner: Pubkey,
    linear_emission: Option<LinearEmission>,
    country_code: u16,
    current_unlock_date: UnixTimestamp,
    // `creator` and `original_unlock_date` help to generate PDA
    creator: Pubkey,
    original_unlock_date: UnixTimestamp,
}

impl Default for Locker {
    fn default() -> Self {
        Self {
            owner: Default::default(),
            linear_emission: Default::default(),
            creator: Default::default(),
            original_unlock_date: Default::default(),
            current_unlock_date: Default::default(),
            country_code: Default::default(),
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub struct LinearEmission {
    emission_start: UnixTimestamp,
    emission_end: UnixTimestamp,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct CreateLockerArgs {
    amount: u64,
    unlock_date: UnixTimestamp,
    country_code: u16,
    linear_emission: Option<LinearEmission>,
    locker_bump: u8,
    vault_bump: u8,
}

#[derive(Accounts)]
#[instruction(args: CreateLockerArgs)]
pub struct CreateLocker<'info> {
    #[account(
        init,
        payer = creator,
        seeds = [
            creator.key().as_ref(),
            args.unlock_date.to_be_bytes().as_ref(),
        ],
        bump = args.locker_bump
    )]
    locker: ProgramAccount<'info, Locker>,
    #[account(signer)]
    creator: AccountInfo<'info>,
    owner: AccountInfo<'info>,
    #[account(signer)]
    funding_wallet_authority: AccountInfo<'info>,
    funding_wallet: Account<'info, TokenAccount>,
    #[account(
        seeds = [
            locker.key().as_ref()
        ],
        bump = args.vault_bump
    )]
    vault_authority: AccountInfo<'info>,
    #[account(
        constraint = vault.mint == funding_wallet.mint
    )]
    vault: Account<'info, TokenAccount>,
    #[account(
        constraint = fee_wallet.mint == funding_wallet.mint
    )]
    fee_wallet: Account<'info, TokenAccount>,

    clock: Sysvar<'info, Clock>,
    system_program: Program<'info, System>,
    token_program: Program<'info, Token>,
}