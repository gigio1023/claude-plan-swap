//! Account switching transaction.
//!
//! Switching touches both secret storage and local metadata. Keeping the flow in
//! one module makes it easier to see the ordering: refresh the current backup,
//! activate the target credential, update state, then reset quota side effects.

use crate::context::AppContext;
use crate::domain::{AccountKind, AccountName, State};
use crate::keychain::Keychain;
use crate::notification;
use crate::storage;
use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::io::{self, IsTerminal, Write};

#[derive(Clone, Copy, Debug)]
pub(crate) struct SwitchOptions {
    pub(crate) yes: bool,
    pub(crate) emit: bool,
}

pub(crate) fn switch_to(
    ctx: &AppContext,
    keychain: &Keychain,
    name: &AccountName,
    options: SwitchOptions,
) -> Result<()> {
    let mut state = storage::load_state(ctx)?;
    let target_entry = state
        .accounts
        .get(name.as_str())
        .cloned()
        .ok_or_else(|| anyhow!("account is not saved: {name}"))?;
    confirm_switch(name, options.yes)?;

    let active_account = match &state.active_account {
        Some(account) => account.clone(),
        None => keychain.detect_active_account()?,
    };

    refresh_current_backup(ctx, keychain, &state, name, &active_account)?;

    let target_credential = keychain
        .read_account(name)
        .with_context(|| format!("failed to read saved credential for {name}"))?;
    let active_credential = keychain.read_active(&active_account).unwrap_or_default();
    if active_credential == target_credential {
        state.active_account = Some(active_account);
        state.current_account = Some(name.to_string());
        storage::save_state(ctx, &state)?;
        if options.emit {
            println!("already using account: {name}");
        }
        return Ok(());
    }

    keychain
        .upsert_active(&active_account, &target_credential)
        .with_context(|| format!("failed to activate account {name}"))?;

    let old_current = state.current_account.clone();
    if old_current.as_deref() != Some(name.as_str()) {
        state.previous_account = old_current;
    }
    state.active_account = Some(active_account);
    state.current_account = Some(name.to_string());
    storage::save_state(ctx, &state)?;

    reset_quota_side_effects(ctx, target_entry.kind);

    if options.emit {
        println!("switched to account: {name}");
        println!("restart Claude Code if the running session does not pick up the new credential");
    }
    Ok(())
}

pub(crate) fn detect_current_account_by_credential(
    keychain: &Keychain,
    state: &mut State,
) -> Result<Option<String>> {
    let account = match &state.active_account {
        Some(account) => account.clone(),
        None => keychain.detect_active_account()?,
    };
    let active = keychain.read_active(&account)?;
    for name in state.accounts.keys() {
        let account_name = AccountName::parse(name)?;
        if let Ok(saved) = keychain.read_account(&account_name)
            && saved == active
        {
            state.active_account = Some(account);
            state.current_account = Some(name.to_string());
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// Refresh the saved credential for the currently active account before switching.
///
/// Claude Code may refresh OAuth tokens while the account is active. Capturing the
/// current credential immediately before switching prevents restoring an older
/// token the next time the user switches back to this account.
fn refresh_current_backup(
    _ctx: &AppContext,
    keychain: &Keychain,
    state: &State,
    target: &AccountName,
    active_account: &str,
) -> Result<()> {
    if let Some(current_account) = state.current_account.as_deref()
        && current_account != target.as_str()
        && state.accounts.contains_key(current_account)
        && let Ok(current_credential) = keychain.read_active(active_account)
    {
        let current_name = AccountName::parse(current_account)?;
        keychain
            .upsert_account(&current_name, &current_credential)
            .with_context(|| {
                format!("failed to update current account backup for {current_account}")
            })?;
    }
    Ok(())
}

/// Reset local quota markers according to the target account kind.
///
/// Team accounts start a fresh quota observation cycle, so the cached rate limit
/// snapshot is removed. Enterprise accounts keep the snapshot because their
/// statusline countdown depends on the team reset timestamp captured earlier.
fn reset_quota_side_effects(ctx: &AppContext, target_kind: AccountKind) {
    match target_kind {
        AccountKind::Enterprise => {
            fs::write(ctx.route_lock_path(), b"").ok();
            notification::clear_notified(ctx).ok();
        }
        AccountKind::Team => {
            fs::remove_file(ctx.route_lock_path()).ok();
            fs::remove_file(ctx.rate_limits_path()).ok();
            notification::clear_notified(ctx).ok();
        }
        AccountKind::Other => {}
    }
}

/// Require an explicit acknowledgement before touching the active credential.
///
/// Statusline auto-switch and Claude Code bang commands use `--yes`; interactive
/// shell use gets a prompt so a typo does not silently replace the Keychain
/// credential read by every running Claude Code session.
fn confirm_switch(name: &AccountName, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        bail!("refusing to switch without confirmation in non-interactive input; pass --yes");
    }

    eprint!("switch to {name}? [y/N] ");
    io::stderr().flush().ok();
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read confirmation")?;
    match answer.trim() {
        "y" | "Y" | "yes" | "YES" => Ok(()),
        _ => bail!("cancelled"),
    }
}
