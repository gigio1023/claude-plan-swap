use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const ACTIVE_SERVICE: &str = "Claude Code-credentials";
const PLAN_SERVICE: &str = "claude-plan-swap";
const APP_HOME_ENV: &str = "CLAUDE_PLAN_SWAP_HOME";
const CLAUDE_HOME_ENV: &str = "CLAUDE_HOME";

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Save the active Claude Code credential as a named plan.
    Setup {
        /// Plan name, for example team, enterprise, team-2, or backup.
        name: String,
        /// Override the plan kind. If omitted, claude auth status is used.
        #[arg(long, value_enum)]
        kind: Option<PlanKind>,
    },
    /// Switch the active Claude Code credential to a saved plan.
    Switch {
        name: String,
        #[arg(short, long)]
        yes: bool,
    },
    /// Switch back to the previous plan.
    Toggle {
        #[arg(short, long)]
        yes: bool,
    },
    /// List saved plans.
    List,
    /// Remove a saved plan.
    Remove { name: String },
    /// Print the current plan name.
    Current,
    /// Show state, config, and cached quota information.
    Status,
    /// Read or update statusline alert settings.
    Config {
        #[arg(long)]
        alert_at: Option<u8>,
        #[arg(long, value_enum)]
        mode: Option<SwapMode>,
    },
    /// Install the Claude Code statusLine wrapper.
    Install,
    /// Remove the Claude Code statusLine wrapper and restore the prior command.
    Uninstall,
    /// Internal command used by Claude Code statusLine.
    Statusline,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum PlanKind {
    Team,
    Enterprise,
    Other,
}

impl PlanKind {
    fn from_subscription(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "team" => Self::Team,
            "enterprise" => Self::Enterprise,
            _ => Self::Other,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Enterprise => "enterprise",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum SwapMode {
    Manual,
    Auto,
}

impl SwapMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    alert_at: u8,
    mode: SwapMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            alert_at: 95,
            mode: SwapMode::Manual,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PlanEntry {
    kind: PlanKind,
    created_at: u64,
    updated_at: u64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct State {
    active_account: Option<String>,
    current_plan: Option<String>,
    previous_plan: Option<String>,
    plans: BTreeMap<String, PlanEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct LimitWindow {
    used_percentage: Option<u8>,
    resets_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RateLimitSnapshot {
    detected_at: u64,
    five_hour: LimitWindow,
    seven_day: LimitWindow,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = AppContext::new()?;

    match cli.command {
        Commands::Setup { name, kind } => cmd_setup(&ctx, &name, kind),
        Commands::Switch { name, yes } => cmd_switch(&ctx, &name, yes),
        Commands::Toggle { yes } => cmd_toggle(&ctx, yes),
        Commands::List => cmd_list(&ctx),
        Commands::Remove { name } => cmd_remove(&ctx, &name),
        Commands::Current => cmd_current(&ctx),
        Commands::Status => cmd_status(&ctx),
        Commands::Config { alert_at, mode } => cmd_config(&ctx, alert_at, mode),
        Commands::Install => cmd_install(&ctx),
        Commands::Uninstall => cmd_uninstall(&ctx),
        Commands::Statusline => cmd_statusline(&ctx),
    }
}

struct AppContext {
    app_dir: PathBuf,
    claude_dir: PathBuf,
}

impl AppContext {
    fn new() -> Result<Self> {
        let app_dir = if let Some(path) = env::var_os(APP_HOME_ENV) {
            PathBuf::from(path)
        } else {
            home_dir()?.join(".config").join("claude-plan-swap")
        };

        let claude_dir = if let Some(path) = env::var_os(CLAUDE_HOME_ENV) {
            PathBuf::from(path)
        } else {
            home_dir()?.join(".claude")
        };

        Ok(Self {
            app_dir,
            claude_dir,
        })
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.app_dir)
            .with_context(|| format!("failed to create {}", self.app_dir.display()))
    }

    fn state_path(&self) -> PathBuf {
        self.app_dir.join("state.json")
    }

    fn config_path(&self) -> PathBuf {
        self.app_dir.join("config.json")
    }

    fn rate_limits_path(&self) -> PathBuf {
        self.app_dir.join("rate-limits.json")
    }

    fn notified_path(&self) -> PathBuf {
        self.app_dir.join("notified.json")
    }

    fn swap_lock_path(&self) -> PathBuf {
        self.app_dir.join("swapped.lock")
    }

    fn inner_statusline_path(&self) -> PathBuf {
        self.app_dir.join("inner-statusline.txt")
    }

    fn settings_path(&self) -> PathBuf {
        self.claude_dir.join("settings.json")
    }
}

fn cmd_setup(ctx: &AppContext, name: &str, kind: Option<PlanKind>) -> Result<()> {
    validate_plan_name(name)?;
    ctx.ensure_dirs()?;

    let mut state = load_state(ctx)?;
    let active_account = match &state.active_account {
        Some(account) => account.clone(),
        None => detect_active_account()?,
    };
    let credential = keychain_read(ACTIVE_SERVICE, &active_account)
        .context("failed to read active Claude Code credential from Keychain")?;
    let kind = kind.unwrap_or_else(|| detect_plan_kind_from_active(&credential));

    keychain_upsert(PLAN_SERVICE, &plan_key(name), &credential)
        .with_context(|| format!("failed to save plan credential for {name}"))?;

    let now = now_epoch();
    let created_at = state
        .plans
        .get(name)
        .map(|entry| entry.created_at)
        .unwrap_or(now);
    state.active_account = Some(active_account);
    state.current_plan = Some(name.to_string());
    state.plans.insert(
        name.to_string(),
        PlanEntry {
            kind,
            created_at,
            updated_at: now,
        },
    );
    save_state(ctx, &state)?;

    println!("saved plan: {name} ({})", kind.as_str());
    Ok(())
}

fn cmd_switch(ctx: &AppContext, name: &str, yes: bool) -> Result<()> {
    switch_to(ctx, name, yes, true)
}

fn cmd_toggle(ctx: &AppContext, yes: bool) -> Result<()> {
    let state = load_state(ctx)?;
    let previous = state
        .previous_plan
        .as_deref()
        .ok_or_else(|| anyhow!("no previous plan recorded"))?;
    if !state.plans.contains_key(previous) {
        bail!("previous plan is not saved: {previous}");
    }
    switch_to(ctx, previous, yes, true)
}

fn cmd_list(ctx: &AppContext) -> Result<()> {
    let state = load_state(ctx)?;
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

fn cmd_remove(ctx: &AppContext, name: &str) -> Result<()> {
    let mut state = load_state(ctx)?;
    if state.current_plan.as_deref() == Some(name) {
        bail!("cannot remove the current plan; switch to another plan first");
    }
    if state.plans.remove(name).is_none() {
        bail!("plan is not saved: {name}");
    }
    keychain_delete(PLAN_SERVICE, &plan_key(name)).ok();
    if state.previous_plan.as_deref() == Some(name) {
        state.previous_plan = None;
    }
    save_state(ctx, &state)?;
    println!("removed plan: {name}");
    Ok(())
}

fn cmd_current(ctx: &AppContext) -> Result<()> {
    let mut state = load_state(ctx)?;
    if let Some(current) = state.current_plan {
        println!("{current}");
        return Ok(());
    }

    if let Some(detected) = detect_current_plan_by_credential(&mut state)? {
        save_state(ctx, &state)?;
        println!("{detected}");
        return Ok(());
    }

    println!("unknown");
    Ok(())
}

fn cmd_status(ctx: &AppContext) -> Result<()> {
    let state = load_state(ctx)?;
    let config = load_config(ctx)?;
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

    if let Some(snapshot) = load_rate_limits(ctx)? {
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

fn cmd_config(ctx: &AppContext, alert_at: Option<u8>, mode: Option<SwapMode>) -> Result<()> {
    ctx.ensure_dirs()?;
    let mut config = load_config(ctx)?;
    if let Some(alert_at) = alert_at {
        if alert_at > 100 {
            bail!("--alert-at must be between 0 and 100");
        }
        config.alert_at = alert_at;
    }
    if let Some(mode) = mode {
        config.mode = mode;
    }
    save_config(ctx, &config)?;
    println!("alert_at={}", config.alert_at);
    println!("mode={}", config.mode.as_str());
    Ok(())
}

fn cmd_install(ctx: &AppContext) -> Result<()> {
    ctx.ensure_dirs()?;
    fs::create_dir_all(&ctx.claude_dir)
        .with_context(|| format!("failed to create {}", ctx.claude_dir.display()))?;

    let settings_path = ctx.settings_path();
    let mut settings = load_settings(&settings_path)?;
    let command = statusline_command()?;

    if let Some(existing) = settings
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        && !is_our_statusline_command(existing)
    {
        fs::write(ctx.inner_statusline_path(), existing)
            .context("failed to save prior statusline command")?;
    }

    settings["statusLine"] = json!({
        "type": "command",
        "command": command,
    });
    save_settings(&settings_path, &settings)?;
    println!("installed Claude Code statusLine wrapper");
    Ok(())
}

fn cmd_uninstall(ctx: &AppContext) -> Result<()> {
    let settings_path = ctx.settings_path();
    let mut settings = load_settings(&settings_path)?;
    let current = settings
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if !is_our_statusline_command(&current) {
        println!("statusLine wrapper is not installed");
        return Ok(());
    }

    let inner_path = ctx.inner_statusline_path();
    if inner_path.exists() {
        let inner = fs::read_to_string(&inner_path)
            .context("failed to read saved prior statusline command")?;
        settings["statusLine"] = json!({
            "type": "command",
            "command": inner.trim(),
        });
    } else if let Some(object) = settings.as_object_mut() {
        object.remove("statusLine");
    }
    save_settings(&settings_path, &settings)?;
    println!("uninstalled Claude Code statusLine wrapper");
    Ok(())
}

fn cmd_statusline(ctx: &AppContext) -> Result<()> {
    let input = read_stdin_to_string()?;
    if input.trim().is_empty() {
        return Ok(());
    }

    let parsed: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };

    let inner_output = run_inner_statusline(ctx, &input);
    let state = load_state(ctx)?;
    let config = load_config(ctx)?;
    let swap_output = statusline_swap_output(ctx, &state, &config, &parsed)?;

    match (swap_output.as_deref(), inner_output.as_deref()) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => println!("{a} | {b}"),
        (Some(a), _) if !a.is_empty() => println!("{a}"),
        (_, Some(b)) if !b.is_empty() => println!("{b}"),
        _ => {}
    }

    Ok(())
}

fn switch_to(ctx: &AppContext, name: &str, yes: bool, emit: bool) -> Result<()> {
    let mut state = load_state(ctx)?;
    let target_entry = state
        .plans
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow!("plan is not saved: {name}"))?;
    confirm_switch(name, yes)?;

    let active_account = match &state.active_account {
        Some(account) => account.clone(),
        None => detect_active_account()?,
    };

    if let Some(current_plan) = state.current_plan.as_deref()
        && current_plan != name
        && let Ok(current_credential) = keychain_read(ACTIVE_SERVICE, &active_account)
        && state.plans.contains_key(current_plan)
    {
        keychain_upsert(PLAN_SERVICE, &plan_key(current_plan), &current_credential)
            .with_context(|| format!("failed to update current plan backup for {current_plan}"))?;
    }

    let target_credential = keychain_read(PLAN_SERVICE, &plan_key(name))
        .with_context(|| format!("failed to read saved credential for {name}"))?;
    let active_credential = keychain_read(ACTIVE_SERVICE, &active_account).unwrap_or_default();
    if active_credential == target_credential {
        state.active_account = Some(active_account);
        state.current_plan = Some(name.to_string());
        save_state(ctx, &state)?;
        if emit {
            println!("already using plan: {name}");
        }
        return Ok(());
    }

    keychain_upsert(ACTIVE_SERVICE, &active_account, &target_credential)
        .with_context(|| format!("failed to activate plan {name}"))?;

    let old_current = state.current_plan.clone();
    if old_current.as_deref() != Some(name) {
        state.previous_plan = old_current;
    }
    state.active_account = Some(active_account);
    state.current_plan = Some(name.to_string());
    save_state(ctx, &state)?;

    match target_entry.kind {
        PlanKind::Enterprise => {
            fs::write(ctx.swap_lock_path(), b"").ok();
            clear_notified(ctx).ok();
        }
        PlanKind::Team => {
            fs::remove_file(ctx.swap_lock_path()).ok();
            fs::remove_file(ctx.rate_limits_path()).ok();
            clear_notified(ctx).ok();
        }
        PlanKind::Other => {}
    }

    if emit {
        println!("switched to plan: {name}");
        println!("restart Claude Code if the running session does not pick up the new credential");
    }
    Ok(())
}

fn statusline_swap_output(
    ctx: &AppContext,
    state: &State,
    config: &Config,
    input: &Value,
) -> Result<Option<String>> {
    let current_plan = match state.current_plan.as_deref() {
        Some(value) => value,
        None => return Ok(None),
    };
    let current_kind = match state.plans.get(current_plan) {
        Some(entry) => entry.kind,
        None => return Ok(None),
    };

    match current_kind {
        PlanKind::Team => statusline_team(ctx, state, config, current_plan, input),
        PlanKind::Enterprise => statusline_enterprise(ctx, state, config, current_plan),
        PlanKind::Other => Ok(Some(current_plan.to_string())),
    }
}

fn statusline_team(
    ctx: &AppContext,
    state: &State,
    config: &Config,
    current_plan: &str,
    input: &Value,
) -> Result<Option<String>> {
    let snapshot = match parse_rate_limits(input) {
        Some(snapshot) => snapshot,
        None => return Ok(None),
    };
    ctx.ensure_dirs()?;
    save_rate_limits(ctx, &snapshot)?;

    let Some((label, pct)) = max_limit(&snapshot) else {
        return Ok(None);
    };

    if pct < config.alert_at {
        return Ok(None);
    }

    if pct >= 100 {
        if config.mode == SwapMode::Auto
            && current_plan == "team"
            && state.plans.contains_key("enterprise")
            && !ctx.swap_lock_path().exists()
        {
            return auto_switch(ctx, "enterprise", "auto switched to enterprise");
        }
        notify_once(
            ctx,
            "team-limit",
            "claude-plan-swap",
            &format!("{current_plan} quota is full. Run claude-plan-swap list."),
        )
        .ok();
        return Ok(Some(format!(
            "{current_plan} LIMIT({label}) -> claude-plan-swap list"
        )));
    }

    notify_once(
        ctx,
        "team-alert",
        "claude-plan-swap",
        &format!("{current_plan} usage is {pct}% ({label}). Prepare to switch."),
    )
    .ok();
    Ok(Some(format!(
        "{current_plan} {pct}%({label}) -> claude-plan-swap list"
    )))
}

fn statusline_enterprise(
    ctx: &AppContext,
    state: &State,
    config: &Config,
    current_plan: &str,
) -> Result<Option<String>> {
    let Some(snapshot) = load_rate_limits(ctx)? else {
        return Ok(Some(current_plan.to_string()));
    };
    let Some((label, reset_at)) = earliest_reset(&snapshot) else {
        return Ok(Some(current_plan.to_string()));
    };
    let remaining = reset_at - now_epoch() as i64;

    if remaining <= 0 {
        if config.mode == SwapMode::Auto
            && current_plan == "enterprise"
            && state.plans.contains_key("team")
            && ctx.swap_lock_path().exists()
        {
            return auto_switch(ctx, "team", "auto switched to team");
        }
        notify_once(
            ctx,
            "reset",
            "claude-plan-swap",
            "team quota reset is complete. Run claude-plan-swap list.",
        )
        .ok();
        return Ok(Some(
            "team quota reset done -> claude-plan-swap list".to_string(),
        ));
    }

    if remaining <= 60 {
        notify_once(
            ctx,
            "reset-1min",
            "claude-plan-swap",
            "team quota resets within 1 minute.",
        )
        .ok();
        return Ok(Some(format!(
            "team reset in {remaining}s({label}) -> claude-plan-swap list"
        )));
    }
    if remaining <= 300 {
        let minutes = (remaining + 59) / 60;
        notify_once(
            ctx,
            "reset-5min",
            "claude-plan-swap",
            &format!("team quota resets in {minutes} minutes."),
        )
        .ok();
        return Ok(Some(format!(
            "team reset in {minutes}m({label}) -> claude-plan-swap list"
        )));
    }

    let minutes = remaining / 60;
    if minutes >= 60 {
        Ok(Some(format!(
            "{current_plan} | team reset in {}h{}m({label})",
            minutes / 60,
            minutes % 60
        )))
    } else {
        Ok(Some(format!(
            "{current_plan} | team reset in {minutes}m({label})"
        )))
    }
}

fn auto_switch(ctx: &AppContext, target: &str, message: &str) -> Result<Option<String>> {
    switch_to(ctx, target, true, false)?;
    Ok(Some(format!("{message}; restart Claude Code")))
}

fn confirm_switch(name: &str, yes: bool) -> Result<()> {
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

fn validate_plan_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        bail!("plan name must be 1-64 characters");
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        bail!("plan name cannot start/end with '-' or contain '--'");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        bail!("plan name must use lowercase letters, digits, and '-' only");
    }
    Ok(())
}

fn detect_plan_kind_from_active(credential: &str) -> PlanKind {
    if let Some(kind) = detect_plan_kind_from_claude_status() {
        return kind;
    }
    detect_plan_kind_from_credential(credential).unwrap_or(PlanKind::Other)
}

fn detect_plan_kind_from_claude_status() -> Option<PlanKind> {
    let output = Command::new("claude")
        .args(["auth", "status"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    value
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(PlanKind::from_subscription)
}

fn detect_plan_kind_from_credential(credential: &str) -> Option<PlanKind> {
    let value: Value = serde_json::from_str(credential).ok()?;
    value
        .pointer("/claudeAiOauth/subscriptionType")
        .and_then(Value::as_str)
        .map(PlanKind::from_subscription)
}

fn detect_current_plan_by_credential(state: &mut State) -> Result<Option<String>> {
    let account = match &state.active_account {
        Some(account) => account.clone(),
        None => detect_active_account()?,
    };
    let active = keychain_read(ACTIVE_SERVICE, &account)?;
    for name in state.plans.keys() {
        if let Ok(saved) = keychain_read(PLAN_SERVICE, &plan_key(name))
            && saved == active
        {
            state.active_account = Some(account);
            state.current_plan = Some(name.clone());
            return Ok(Some(name.clone()));
        }
    }
    Ok(None)
}

fn parse_rate_limits(input: &Value) -> Option<RateLimitSnapshot> {
    let rate_limits = input.get("rate_limits")?;
    Some(RateLimitSnapshot {
        detected_at: now_epoch(),
        five_hour: LimitWindow {
            used_percentage: value_to_u8(rate_limits.pointer("/five_hour/used_percentage")),
            resets_at: value_to_i64(rate_limits.pointer("/five_hour/resets_at")),
        },
        seven_day: LimitWindow {
            used_percentage: value_to_u8(rate_limits.pointer("/seven_day/used_percentage")),
            resets_at: value_to_i64(rate_limits.pointer("/seven_day/resets_at")),
        },
    })
}

fn max_limit(snapshot: &RateLimitSnapshot) -> Option<(&'static str, u8)> {
    let mut best: Option<(&'static str, u8)> = None;
    if let Some(pct) = snapshot.five_hour.used_percentage {
        best = Some(("5h", pct));
    }
    if let Some(pct) = snapshot.seven_day.used_percentage
        && best.is_none_or(|(_, current)| pct > current)
    {
        best = Some(("7d", pct));
    }
    best
}

fn earliest_reset(snapshot: &RateLimitSnapshot) -> Option<(&'static str, i64)> {
    let mut best: Option<(&'static str, i64)> = None;
    if let Some(reset) = snapshot.five_hour.resets_at {
        best = Some(("5h", reset));
    }
    if let Some(reset) = snapshot.seven_day.resets_at
        && best.is_none_or(|(_, current)| reset < current)
    {
        best = Some(("7d", reset));
    }
    best
}

fn value_to_u8(value: Option<&Value>) -> Option<u8> {
    match value? {
        Value::Number(number) => number.as_u64().and_then(|value| u8::try_from(value).ok()),
        Value::String(text) => text.parse::<u8>().ok(),
        _ => None,
    }
}

fn value_to_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

fn load_state(ctx: &AppContext) -> Result<State> {
    load_json_or_default(&ctx.state_path())
}

fn save_state(ctx: &AppContext, state: &State) -> Result<()> {
    ctx.ensure_dirs()?;
    save_json(&ctx.state_path(), state)
}

fn load_config(ctx: &AppContext) -> Result<Config> {
    load_json_or_default(&ctx.config_path())
}

fn save_config(ctx: &AppContext, config: &Config) -> Result<()> {
    ctx.ensure_dirs()?;
    save_json(&ctx.config_path(), config)
}

fn load_rate_limits(ctx: &AppContext) -> Result<Option<RateLimitSnapshot>> {
    if !ctx.rate_limits_path().exists() {
        return Ok(None);
    }
    let snapshot = fs::read_to_string(ctx.rate_limits_path())
        .context("failed to read rate limit snapshot")
        .and_then(|text| {
            serde_json::from_str(&text).context("failed to parse rate limit snapshot")
        })?;
    Ok(Some(snapshot))
}

fn save_rate_limits(ctx: &AppContext, snapshot: &RateLimitSnapshot) -> Result<()> {
    save_json(&ctx.rate_limits_path(), snapshot)
}

fn load_json_or_default<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{text}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn load_settings(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_settings(path: &Path, settings: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(settings)?),
    )
    .with_context(|| format!("failed to write {}", path.display()))
}

fn keychain_read(service: &str, account: &str) -> Result<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .output()
        .with_context(|| format!("failed to run security for service {service}"))?;
    if !output.status.success() {
        bail!("security find-generic-password failed for service {service} account {account}");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_string())
}

fn keychain_upsert(service: &str, account: &str, password: &str) -> Result<()> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            service,
            "-a",
            account,
            "-w",
        ])
        .arg(password)
        .output()
        .with_context(|| format!("failed to run security for service {service}"))?;
    if !output.status.success() {
        bail!(
            "security add-generic-password failed for service {service} account {account}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn keychain_delete(service: &str, account: &str) -> Result<()> {
    let output = Command::new("security")
        .args(["delete-generic-password", "-s", service, "-a", account])
        .output()
        .with_context(|| format!("failed to run security for service {service}"))?;
    if !output.status.success() {
        bail!("security delete-generic-password failed");
    }
    Ok(())
}

fn detect_active_account() -> Result<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", ACTIVE_SERVICE])
        .output()
        .context("failed to run security to detect active Claude Code account")?;
    if !output.status.success() {
        bail!("Claude Code credential not found in Keychain; log in with claude first");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_keychain_account(&text)
        .ok_or_else(|| anyhow!("failed to parse active Keychain account for Claude Code"))
}

fn parse_keychain_account(output: &str) -> Option<String> {
    for line in output.lines() {
        if !line.contains("\"acct\"") {
            continue;
        }
        let (_, value) = line.split_once("=\"")?;
        return Some(value.trim_end_matches('"').to_string());
    }
    None
}

fn plan_key(name: &str) -> String {
    format!("plan:{name}")
}

fn read_stdin_to_string() -> Result<String> {
    io::read_to_string(io::stdin()).context("failed to read stdin")
}

fn run_inner_statusline(ctx: &AppContext, input: &str) -> Option<String> {
    let command = fs::read_to_string(ctx.inner_statusline_path()).ok()?;
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    let mut child = Command::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn notify_once(ctx: &AppContext, key: &str, title: &str, message: &str) -> Result<()> {
    ctx.ensure_dirs()?;
    let mut notified: BTreeSet<String> = load_json_or_default(&ctx.notified_path())?;
    if notified.contains(key) {
        return Ok(());
    }
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape_applescript(message),
        escape_applescript(title)
    );
    Command::new("osascript")
        .args(["-e", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();
    notified.insert(key.to_string());
    save_json(&ctx.notified_path(), &notified)
}

fn clear_notified(ctx: &AppContext) -> Result<()> {
    if ctx.notified_path().exists() {
        fs::remove_file(ctx.notified_path()).context("failed to clear notification state")?;
    }
    Ok(())
}

fn statusline_command() -> Result<String> {
    let exe = env::current_exe().context("failed to find current executable")?;
    Ok(format!("{} statusline", shell_quote(&exe)))
}

fn is_our_statusline_command(command: &str) -> bool {
    command.contains("claude-plan-swap") && command.contains("statusline")
}

fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', r#"'\''"#))
}

fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn display_pct(value: Option<u8>) -> String {
    value
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "-".to_string())
}

fn display_ts(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keychain_account() {
        let text = r#"
keychain: "/Users/me/Library/Keychains/login.keychain-db"
class: "genp"
attributes:
    "acct"<blob>="me@example.com"
    "svce"<blob>="Claude Code-credentials"
"#;
        assert_eq!(
            parse_keychain_account(text).as_deref(),
            Some("me@example.com")
        );
    }

    #[test]
    fn validates_plan_names() {
        assert!(validate_plan_name("team").is_ok());
        assert!(validate_plan_name("team-2").is_ok());
        assert!(validate_plan_name("Team").is_err());
        assert!(validate_plan_name("-team").is_err());
        assert!(validate_plan_name("team-").is_err());
        assert!(validate_plan_name("team--2").is_err());
    }

    #[test]
    fn parses_rate_limits_from_statusline_json() {
        let input = json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 91, "resets_at": 1000 },
                "seven_day": { "used_percentage": "12", "resets_at": "2000" }
            }
        });
        let parsed = parse_rate_limits(&input).unwrap();
        assert_eq!(parsed.five_hour.used_percentage, Some(91));
        assert_eq!(parsed.five_hour.resets_at, Some(1000));
        assert_eq!(parsed.seven_day.used_percentage, Some(12));
        assert_eq!(parsed.seven_day.resets_at, Some(2000));
    }

    #[test]
    fn picks_max_usage_and_earliest_reset() {
        let snapshot = RateLimitSnapshot {
            detected_at: 1,
            five_hour: LimitWindow {
                used_percentage: Some(80),
                resets_at: Some(500),
            },
            seven_day: LimitWindow {
                used_percentage: Some(95),
                resets_at: Some(1000),
            },
        };
        assert_eq!(max_limit(&snapshot), Some(("7d", 95)));
        assert_eq!(earliest_reset(&snapshot), Some(("5h", 500)));
    }

    #[test]
    fn quotes_shell_paths() {
        assert_eq!(
            shell_quote(Path::new("/tmp/a b/plan's/bin")),
            "'/tmp/a b/plan'\\''s/bin'"
        );
    }
}
