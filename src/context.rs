//! Filesystem context for Claude Plan Swap.
//!
//! Paths are centralized here so command logic can refer to semantic locations
//! rather than rebuilding path strings across modules.

use anyhow::{Context, Result, anyhow};
use std::env;
use std::fs;
use std::path::PathBuf;

const APP_HOME_ENV: &str = "CLAUDE_PLAN_SWAP_HOME";
const CLAUDE_HOME_ENV: &str = "CLAUDE_HOME";

#[derive(Clone, Debug)]
pub(crate) struct AppContext {
    pub(crate) app_dir: PathBuf,
    pub(crate) claude_dir: PathBuf,
}

impl AppContext {
    pub(crate) fn new() -> Result<Self> {
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

    pub(crate) fn ensure_app_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.app_dir)
            .with_context(|| format!("failed to create {}", self.app_dir.display()))
    }

    pub(crate) fn ensure_claude_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.claude_dir)
            .with_context(|| format!("failed to create {}", self.claude_dir.display()))
    }

    pub(crate) fn state_path(&self) -> PathBuf {
        self.app_dir.join("state.json")
    }

    pub(crate) fn config_path(&self) -> PathBuf {
        self.app_dir.join("config.json")
    }

    pub(crate) fn rate_limits_path(&self) -> PathBuf {
        self.app_dir.join("rate-limits.json")
    }

    pub(crate) fn notified_path(&self) -> PathBuf {
        self.app_dir.join("notified.json")
    }

    pub(crate) fn swap_lock_path(&self) -> PathBuf {
        self.app_dir.join("swapped.lock")
    }

    pub(crate) fn inner_statusline_path(&self) -> PathBuf {
        self.app_dir.join("inner-statusline.txt")
    }

    pub(crate) fn settings_path(&self) -> PathBuf {
        self.claude_dir.join("settings.json")
    }
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}
