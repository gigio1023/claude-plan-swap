# Claude Quota Router

Claude Quota Router switches saved Claude Code accounts on macOS and shows quota timing in the statusline.

## Scenario

Use it when a team account is near quota and an enterprise account should run until the team quota resets.

```text
team account
  -> quota alert in statusline
  -> switch to enterprise
  -> team reset countdown
  -> switch back to team
```

## Install

Build the binary and place it on `PATH`.

```bash
cargo build --release
cp target/release/claude-quota-router ~/.local/bin/
```

## Setup

Save each Claude Code login once.

1. Log in with the team account.

```bash
claude-quota-router setup team --kind team
```

2. Log out, log in with the enterprise account, then save it.

```bash
claude-quota-router setup enterprise --kind enterprise
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
claude-quota-router switch enterprise --yes
claude-quota-router switch team --yes
claude-quota-router toggle --yes
claude-quota-router list
claude-quota-router status
```

## Auto Mode

Auto mode only handles the conventional `team` and `enterprise` account names.

```bash
claude-quota-router config --mode auto
```

| State | Action |
|---|---|
| `team` reaches 100% | switch to `enterprise` |
| cached team reset time passes | switch to `team` |

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
