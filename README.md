# gcal

English | [日本語](README.ja.md)

A lightweight Google Calendar client for the terminal, written in Rust.

- **Multiple accounts**: view events from all your Google accounts in one list
- **CLI and TUI**: script it with subcommands, or run `gcal` with no arguments for an interactive UI
- **Easy OAuth**: `gcal login <name>` opens your browser. Click "Allow" and you're done, with no codes to copy and paste
- **Small**: a single ~2MB binary with no runtime dependencies (TLS is built in)

## Install

```sh
cargo install --git https://github.com/polidog/gcal
```

## Setup

Google requires every app to use its own OAuth client, so you need to create one once (about 10 minutes).

1. [Create a project](https://console.cloud.google.com/projectcreate) in Google Cloud Console (any name).
2. [Enable the Google Calendar API](https://console.cloud.google.com/apis/library/calendar-json.googleapis.com).
3. Open **Google Auth Platform** in the menu:
   - **Branding**: enter an app name and your email.
   - **Audience**: choose "External" and add every Google account you want to use as a **test user**.
4. **Clients** → **Create client** → type **Desktop app**, then download the JSON.
5. Register it and log in:

```sh
gcal init ~/Downloads/client_secret_xxx.json
gcal login work
gcal login private   # repeat for each account
```

`gcal init` only copies the client ID and secret into `~/.config/gcal/client.json`. You can delete the downloaded JSON afterwards.

> While the app's audience is in **Testing**, Google expires logins after 7 days. Click **Publish app** to avoid that. Google will show an "unverified app" warning, which is fine for personal use.

## Usage

### CLI

```sh
gcal list                      # next 7 days, all accounts
gcal list -d 14 -a work        # 14 days, one account
gcal list --all                # include events you declined
gcal list --ids                # show event IDs (for edit/delete)

gcal add "Meeting" "2026-09-26 10:00" "2026-09-26 11:00" -a work
gcal add "Holiday" 2026-09-28 2026-09-29        # all-day (end date is inclusive)

gcal edit <ID> --start "2026-09-26 14:00" --end "2026-09-26 15:00" -a work
gcal edit <ID> --title "New title" -a work
gcal delete <ID> -a work

gcal accounts                  # list accounts
gcal logout work               # remove an account
```

`-a` can be omitted when you have only one account.

### TUI

Run `gcal` (or `gcal tui`).

| Key | Action |
|---|---|
| `j` / `k` | Move |
| `h` / `l` | Previous / next week |
| `t` | Today |
| `Tab` | Switch account (all → each) |
| `a` | Add event (to the account shown) |
| `e` | Edit event |
| `d` | Delete event (confirm with `y`) |
| `x` | Show / hide declined events |
| `o` | Open in browser |
| `r` | Reload |
| `q` | Quit |

In the add/edit form, use `Tab` / `↑↓` to move between fields, `Enter` to save and `Esc` to cancel.

## Files

| Path | Contents |
|---|---|
| `~/.config/gcal/client.json` | OAuth client (from `gcal init`) |
| `~/.config/gcal/accounts/<name>.json` | Tokens per account (mode 0600) |

`$XDG_CONFIG_HOME` is respected.

## Limitations

- Only each account's **primary** calendar is shown.
- Editing or deleting a recurring event affects only that single occurrence.
