//! Deterministic account selection for automatic routing.
//!
//! Claude Quota Router supports many saved accounts of the same kind. This
//! module keeps the selection policy small and testable: route from the active
//! team account to the first saved enterprise account, then return to the exact
//! team account recorded in the route lock after the cached reset time passes.

use crate::domain::{AccountKind, RouteLock, State};

pub(crate) fn enterprise_target(state: &State, current_account: &str) -> Option<String> {
    first_account_of_kind(state, AccountKind::Enterprise, current_account)
}

pub(crate) fn locked_team_target(
    state: &State,
    current_account: &str,
    lock: &RouteLock,
) -> Option<String> {
    let source = lock.source_account.as_str();
    if source == current_account {
        return None;
    }
    if account_kind(state, source) == Some(AccountKind::Team) {
        return Some(source.to_string());
    }
    None
}

pub(crate) fn previous_team_target(state: &State, current_account: &str) -> Option<String> {
    let previous = state.previous_account.as_deref()?;
    if previous == current_account {
        return None;
    }
    if account_kind(state, previous) == Some(AccountKind::Team) {
        return Some(previous.to_string());
    }
    None
}

fn first_account_of_kind(
    state: &State,
    kind: AccountKind,
    excluded_account: &str,
) -> Option<String> {
    state
        .accounts
        .iter()
        .find(|(name, entry)| name.as_str() != excluded_account && entry.kind == kind)
        .map(|(name, _)| name.clone())
}

fn account_kind(state: &State, account: &str) -> Option<AccountKind> {
    state.accounts.get(account).map(|entry| entry.kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountEntry;
    use std::collections::BTreeMap;

    fn state_with(accounts: &[(&str, AccountKind)]) -> State {
        let mut entries = BTreeMap::new();
        for (name, kind) in accounts {
            entries.insert(
                (*name).to_string(),
                AccountEntry {
                    kind: *kind,
                    created_at: 1,
                    updated_at: 1,
                },
            );
        }
        State {
            active_account: Some("active".to_string()),
            current_account: None,
            previous_account: Some("team-secondary".to_string()),
            accounts: entries,
        }
    }

    #[test]
    fn picks_first_enterprise_account_by_saved_name() {
        let state = state_with(&[
            ("enterprise-b", AccountKind::Enterprise),
            ("team-main", AccountKind::Team),
            ("enterprise-a", AccountKind::Enterprise),
        ]);

        assert_eq!(
            enterprise_target(&state, "team-main").as_deref(),
            Some("enterprise-a")
        );
    }

    #[test]
    fn returns_to_locked_team_account() {
        let state = state_with(&[
            ("enterprise-a", AccountKind::Enterprise),
            ("team-main", AccountKind::Team),
            ("team-secondary", AccountKind::Team),
        ]);
        let lock = RouteLock {
            source_account: "team-secondary".to_string(),
            routed_account: "enterprise-a".to_string(),
            created_at: 10,
        };

        assert_eq!(
            locked_team_target(&state, "enterprise-a", &lock).as_deref(),
            Some("team-secondary")
        );
    }

    #[test]
    fn ignores_route_lock_when_source_is_not_team() {
        let state = state_with(&[
            ("enterprise-a", AccountKind::Enterprise),
            ("other-a", AccountKind::Other),
            ("team-main", AccountKind::Team),
        ]);
        let lock = RouteLock {
            source_account: "other-a".to_string(),
            routed_account: "enterprise-a".to_string(),
            created_at: 10,
        };

        assert!(locked_team_target(&state, "enterprise-a", &lock).is_none());
    }
}
