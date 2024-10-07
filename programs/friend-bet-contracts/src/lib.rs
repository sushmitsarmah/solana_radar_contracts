use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// This is your program's public key and it will update
// automatically when you build the project.
declare_id!("ADK4T3Mn5MrMzbeykG8qxKNJke5iKnBAnN289pSvs7WQ");

#[program]
mod hello_anchor {
    use super::*;
    pub fn initialize(ctx: Context<Initialize>, data: u64) -> Result<()> {
        ctx.accounts.new_account.data = data;
        msg!("Changed data to: {}!", data); // Message will show up in the tx logs
        Ok(())
    }

    pub fn create_bet(ctx: Context<CreateBet>, question: String, expiry_time: i64) -> Result<()> {
        let bet = &mut ctx.accounts.bet;
        bet.creator = ctx.accounts.creator.key();
        bet.question = question;
        bet.expiry_time = expiry_time;
        bet.total_stake = 0;
        bet.is_resolved = false;
        bet.token_mint = ctx.accounts.token_mint.key();
        Ok(())
    }

    pub fn place_stake(ctx: Context<PlaceStake>, amount: u64, choice: bool) -> Result<()> {
        let bet = &mut ctx.accounts.bet;

        require!(!bet.is_resolved, BettingError::BetAlreadyResolved);
        require!(
            Clock::get()?.unix_timestamp < bet.expiry_time,
            BettingError::BetExpired
        );

        bet.stakes.push(Stake {
            staker: ctx.accounts.staker.key(),
            amount,
            choice,
        });
        bet.total_stake += amount;

        // Transfer tokens from staker to bet vault
        let cpi_accounts = token::Transfer {
            from: ctx.accounts.staker_token_account.to_account_info(),
            to: ctx.accounts.bet_vault.to_account_info(),
            authority: ctx.accounts.staker.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        Ok(())
    }

    pub fn resolve_bet(ctx: Context<ResolveBet>, outcome: bool) -> Result<()> {
        let bet = &mut ctx.accounts.bet;

        require!(!bet.is_resolved, BettingError::BetAlreadyResolved);
        require!(
            Clock::get()?.unix_timestamp >= bet.expiry_time,
            BettingError::BetNotExpired
        );
        require!(
            bet.creator == ctx.accounts.creator.key(),
            BettingError::UnauthorizedResolver
        );

        bet.is_resolved = true;
        bet.outcome = outcome;

        // Calculate winnings
        let mut total_winning_stake = 0;
        for stake in bet.stakes.iter() {
            if stake.choice == outcome {
                total_winning_stake += stake.amount;
            }
        }

        // Distribute winnings
        for stake in bet.stakes.iter() {
            if stake.choice == outcome {
                let winnings =
                    (stake.amount as u128 * bet.total_stake as u128) / total_winning_stake as u128;

                // Transfer winnings from bet vault to winner
                let seeds = &[
                    bet.to_account_info().key.as_ref(),
                    // &[*ctx.bumps.get("bet_vault").unwrap()],
                    &[ctx.bumps.bet_vault],
                ];
                let signer = [&seeds[..]];

                let cpi_accounts = token::Transfer {
                    from: ctx.accounts.bet_vault.to_account_info(),
                    to: ctx.accounts.winner_token_account.to_account_info(),
                    authority: ctx.accounts.bet_vault.to_account_info(),
                };
                let cpi_program = ctx.accounts.token_program.to_account_info();
                let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, &signer);
                token::transfer(cpi_ctx, winnings as u64)?;
            }
        }

        Ok(())
    }
}

#[derive(Accounts)]
pub struct CreateBet<'info> {
    #[account(init, payer = creator, space = 8 + 32 + 256 + 8 + 1 + 1 + 32 + (32 + 8 + 1) * 10)]
    pub bet: Account<'info, Bet>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub token_mint: Account<'info, Mint>,
    #[account(
        init,
        payer = creator,
        associated_token::mint = token_mint,
        associated_token::authority = bet
    )]
    pub bet_vault: Account<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct PlaceStake<'info> {
    #[account(mut)]
    pub bet: Account<'info, Bet>,
    #[account(mut)]
    pub staker: Signer<'info>,
    #[account(mut, constraint = staker_token_account.mint == bet.token_mint)]
    pub staker_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = bet_vault.mint == bet.token_mint)]
    pub bet_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ResolveBet<'info> {
    #[account(mut)]
    pub bet: Account<'info, Bet>,
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: This account is not read from or written to
    pub winner: AccountInfo<'info>,
    #[account(mut, constraint = winner_token_account.mint == bet.token_mint)]
    pub winner_token_account: Account<'info, TokenAccount>,
    // #[account(mut, constraint = bet_vault.mint == bet.token_mint)]
    #[account(
        mut,
        seeds = [bet.to_account_info().key.as_ref()],
        bump,
        constraint = bet_vault.mint == bet.token_mint
    )]
    pub bet_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    // We must specify the space in order to initialize an account.
    // First 8 bytes are default account discriminator,
    // next 8 bytes come from NewAccount.data being type u64.
    // (u64 = 64 bits unsigned integer = 8 bytes)
    #[account(init, payer = signer, space = 8 + 8)]
    pub new_account: Account<'info, NewAccount>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct NewAccount {
    data: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct Stake {
    pub staker: Pubkey,
    pub amount: u64,
    pub choice: bool,
}

#[account]
pub struct Bet {
    pub creator: Pubkey,
    pub question: String,
    pub expiry_time: i64,
    pub total_stake: u64,
    pub is_resolved: bool,
    pub outcome: bool,
    pub token_mint: Pubkey,
    pub stakes: Vec<Stake>,
}

#[error_code]
pub enum BettingError {
    #[msg("This bet has already been resolved")]
    BetAlreadyResolved,
    #[msg("This bet has expired")]
    BetExpired,
    #[msg("This bet has not yet expired")]
    BetNotExpired,
    #[msg("Only the creator can resolve this bet")]
    UnauthorizedResolver,
}
