//! Application use cases.
//!
//! The `App` type is the orchestration layer. It coordinates modules with
//! side effects but leaves their implementation details in their own files.

use crate::claude;
use crate::cli::Commands;
use crate::context::AppContext;
use crate::domain::{Config, PlanEntry, PlanName, SwapMode};
use crate::keychain::Keychain;
use crate::settings;
use crate::statusline;
use crate::storage;
use crate::switcher::{self, SwitchOptions};
use crate::time::now_epoch;
use crate::util::{display_pct, display_ts};
use anyhow::{Context, Result, anyhow, bail};

pub(crate) struct App {
    ctx: AppContext,
    keychain: Keychain,
}

impl App {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            ctx: AppContext::new()?,
            keychain: Keychain,
        })
    }

    pub(crate) fn handle(&self, command: Commands) -> Result<()> {
        match command {
            Commands::Setup { name, kind } => self.setup(&name, kind),
            Commands::Switch { name, yes } => self.switch(&name, yes),
            Commands::Toggle { yes } => self.toggle(yes),
            Commands::List => self.list(),
            Commands::Remove { name } => self.remove(&name),
            Commands::Current => self.current(),
            Commands::Status => self.status(),
            Commands::Config { alert_at, mode } => self.config(alert_at, mode),
            Commands::Install => settings::install_statusline(&self.ctx),
            Commands::Uninstall => settings::uninstall_statusline(&self.ctx),
            Commands::Statusline => statusline::handle(&self.ctx, &self.keychain),
        }
    }

    fn setup(&self, name: &str, kind: Option<crate::domain::PlanKind>) -> Result<()> {
        let name = PlanName::parse(name)?;
        self.ctx.ensure_app_dir()?;

        let mut state = storage::load_state(&self.ctx)?;
        // The active Keychain account is stable for a given Claude Code
        // installation. Cache it after first detection so later commands do not
        // need to parse Keychain metadata unless the state file is missing.
        let active_account = match &state.active_account {
            Some(account) => account.clone(),
            None => self.keychain.detect_active_account()?,
        };
        let credential = self
            .keychain
            .read_active(&active_account)
            .context("failed to read active Claude Code credential from Keychain")?;
        // Prefer an explicit CLI override for import/migration cases. Otherwise
        // ask Claude Code first and fall back to the credential JSON shape.
        let kind =
            kind.unwrap_or_else(|| claude::detect_plan_kind_from_active_credential(&credential));

        self.keychain
            .upsert_plan(&name, &credential)
            .with_context(|| format!("failed to save plan credential for {name}"))?;

        let now = now_epoch();
        let created_at = state
            .plans
            .get(name.as_str())
            .map(|entry| entry.created_at)
            .unwrap_or(now);
        state.active_account = Some(active_account);
        state.current_plan = Some(name.to_string());
        state.plans.insert(
            name.clone().into_string(),
            PlanEntry {
                kind,
                created_at,
                updated_at: now,
            },
        );
        storage::save_state(&self.ctx, &state)?;

        println!("saved plan: {name} ({})", kind.as_str());
        Ok(())
    }

    fn switch(&self, name: &str, yes: bool) -> Result<()> {
        let name = PlanName::parse(name)?;
        switcher::switch_to(
            &self.ctx,
            &self.keychain,
            &name,
            SwitchOptions { yes, emit: true },
        )
    }

    fn toggle(&self, yes: bool) -> Result<()> {
        let state = storage::load_state(&self.ctx)?;
        let previous = state
            .previous_plan
            .as_deref()
            .ok_or_else(|| anyhow!("no previous plan recorded"))?;
        if !state.plans.contains_key(previous) {
            bail!("previous plan is not saved: {previous}");
        }
        self.switch(previous, yes)
    }

    fn list(&self) -> Result<()> {
        let state = storage::load_state(&self.ctx)?;
        if state.plans.is_empty() {
            println!("no plans saved");
            return Ok(());
        }

        for (name, entry) in &state.plans {
            let marker = if state.current_plan.as_deref() == Some(name.as_str()) {
                "*"
            } else {
                " "
            };
            println!("{marker} {name}\t{}", entry.kind.as_str());
        }
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        let name = PlanName::parse(name)?;
        let mut state = storage::load_state(&self.ctx)?;
        if state.current_plan.as_deref() == Some(name.as_str()) {
            bail!("cannot remove the current plan; switch to another plan first");
        }
        if state.plans.remove(name.as_str()).is_none() {
            bail!("plan is not saved: {name}");
        }
        self.keychain.delete_plan(&name).ok();
        if state.previous_plan.as_deref() == Some(name.as_str()) {
            state.previous_plan = None;
        }
        storage::save_state(&self.ctx, &state)?;
        println!("removed plan: {name}");
        Ok(())
    }

    fn current(&self) -> Result<()> {
        let mut state = storage::load_state(&self.ctx)?;
        if let Some(current) = state.current_plan {
            println!("{current}");
            return Ok(());
        }

        if let Some(detected) =
            switcher::detect_current_plan_by_credential(&self.keychain, &mut state)?
        {
            storage::save_state(&self.ctx, &state)?;
            println!("{detected}");
            return Ok(());
        }

        println!("unknown");
        Ok(())
    }

    fn status(&self) -> Result<()> {
        let state = storage::load_state(&self.ctx)?;
        let config = storage::load_config(&self.ctx)?;
        println!(
            "current: {}",
            state.current_plan.as_deref().unwrap_or("unknown")
        );
        println!(
            "previous: {}",
            state.previous_plan.as_deref().unwrap_or("none")
        );
        println!(
            "active keychain account: {}",
            state.active_account.as_deref().unwrap_or("unknown")
        );
        println!(
            "config: alert_at={} mode={}",
            config.alert_at,
            config.mode.as_str()
        );
        println!("plans: {}", state.plans.len());

        if let Some(snapshot) = storage::load_rate_limits(&self.ctx)? {
            println!(
                "quota: 5h={} reset={} / 7d={} reset={}",
                display_pct(snapshot.five_hour.used_percentage),
                display_ts(snapshot.five_hour.resets_at),
                display_pct(snapshot.seven_day.used_percentage),
                display_ts(snapshot.seven_day.resets_at)
            );
        } else {
            println!("quota: none");
        }

        Ok(())
    }

    fn config(&self, alert_at: Option<u8>, mode: Option<SwapMode>) -> Result<()> {
        self.ctx.ensure_app_dir()?;
        let mut config = storage::load_config(&self.ctx)?;
        if let Some(alert_at) = alert_at {
            if alert_at > 100 {
                bail!("--alert-at must be between 0 and 100");
            }
            config.alert_at = alert_at;
        }
        if let Some(mode) = mode {
            config.mode = mode;
        }
        storage::save_config(&self.ctx, &config)?;
        print_config(&config);
        Ok(())
    }
}

fn print_config(config: &Config) {
    println!("alert_at={}", config.alert_at);
    println!("mode={}", config.mode.as_str());
}
