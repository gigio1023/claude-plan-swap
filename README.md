# Claude Quota Router

Claude Quota Router switches saved Claude Code accounts on macOS and shows quota timing in the statusline.

## Scenario

Use it when one team account is near quota and any saved enterprise account should run until that team account resets.

```text
team-main account
  -> quota alert in statusline
  -> switch to enterprise-main
  -> team-main reset countdown
  -> switch back to team-main
```

## Install

Build the binary and place it on `PATH`.

```bash
cargo build --release
cp target/release/claude-quota-router ~/.local/bin/
```

## Setup

Save each Claude Code login once. Use any lowercase account name.

1. Log in with a team account.

```bash
claude-quota-router setup team-main --kind team
claude-quota-router setup team-side --kind team
```

2. Log out, log in with an enterprise account, then save it.

```bash
claude-quota-router setup enterprise-main --kind enterprise
claude-quota-router setup enterprise-backup --kind enterprise
```

3. Install the statusline wrapper.

```bash
claude-quota-router install
```

4. Set the alert threshold.

```bash
claude-quota-router config --alert-at 95 --mode manual
```

## Daily Use

Switch accounts from a shell or from Claude Code with `!`.

```bash
claude-quota-router switch enterprise-main --yes
claude-quota-router switch team-side --yes
claude-quota-router toggle --yes
claude-quota-router list
claude-quota-router status
```

## Auto Mode

Auto mode uses account kind, not account name.

```bash
claude-quota-router config --mode auto
```

| State | Action |
|---|---|
| current `team` account reaches 100% | switch to first `enterprise` account by name |
| cached reset time passes | switch back to the source `team` account |

## Storage

| Data | Location |
|---|---|
| Saved account credentials | macOS Keychain service `claude-quota-router` |
| Active Claude Code credential | macOS Keychain service `Claude Code-credentials` |
| Account metadata | `~/.config/claude-quota-router/state.json` |
| Quota cache | `~/.config/claude-quota-router/rate-limits.json` |

## Verify

Run the checks before publishing a change.

```bash
cargo test
cargo clippy -- -D warnings
cargo build --release
```
