# Claude Plan Swap

Claude Plan Swap switches saved Claude Code plan credentials on macOS and shows quota timing in the statusline.

## Scenario

Use it when a team plan is near quota and an enterprise plan should run until the team quota resets.

```text
team plan
  -> quota alert in statusline
  -> switch to enterprise
  -> team reset countdown
  -> switch back to team
```

## Install

Build the binary and place it on `PATH`.

```bash
cargo build --release
cp target/release/claude-plan-swap ~/.local/bin/
```

## Setup

Save each Claude Code login once.

1. Log in with the team account.

```bash
claude-plan-swap setup team --kind team
```

2. Log out, log in with the enterprise account, then save it.

```bash
claude-plan-swap setup enterprise --kind enterprise
```

3. Install the statusline wrapper.

```bash
claude-plan-swap install
```

4. Set the alert threshold.

```bash
claude-plan-swap config --alert-at 95 --mode manual
```

## Daily Use

Switch plans from a shell or from Claude Code with `!`.

```bash
claude-plan-swap switch enterprise --yes
claude-plan-swap switch team --yes
claude-plan-swap toggle --yes
claude-plan-swap list
claude-plan-swap status
```

## Auto Mode

Auto mode only handles the conventional `team` and `enterprise` plan names.

```bash
claude-plan-swap config --mode auto
```

| State | Action |
|---|---|
| `team` reaches 100% | switch to `enterprise` |
| cached team reset time passes | switch to `team` |

## Storage

| Data | Location |
|---|---|
| Saved plan credentials | macOS Keychain service `claude-plan-swap` |
| Active Claude Code credential | macOS Keychain service `Claude Code-credentials` |
| Plan metadata | `~/.config/claude-plan-swap/state.json` |
| Quota cache | `~/.config/claude-plan-swap/rate-limits.json` |

## Verify

Run the checks before publishing a change.

```bash
cargo test
cargo clippy -- -D warnings
cargo build --release
```
