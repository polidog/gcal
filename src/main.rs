mod api;
mod auth;
mod tui;

use anyhow::{Result, bail};
use api::{Patch, When};
use chrono::Local;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// 軽量 Google カレンダー CLI/TUI。引数なしで TUI が起動します
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Google Cloud で作った「デスクトップアプリ」の client_secret.json を登録
    Init { path: PathBuf },
    /// アカウントを追加（ブラウザが開きます）
    Login { name: String },
    /// アカウントを削除
    Logout { name: String },
    /// 登録済みアカウント一覧
    Accounts,
    /// 予定一覧（全アカウントをまとめて表示）
    List {
        #[arg(short, long, default_value_t = 7)]
        days: i64,
        #[arg(short, long)]
        account: Option<String>,
        /// 不参加と返事した予定も表示
        #[arg(long)]
        all: bool,
        /// 編集・削除に使う予定 ID も表示
        #[arg(long)]
        ids: bool,
    },
    /// 予定を追加  例: gcal add 打ち合わせ "2026-09-26 10:00" "2026-09-26 11:00" -a work
    Add {
        title: String,
        /// "YYYY-MM-DD HH:MM" または終日なら YYYY-MM-DD
        start: String,
        end: String,
        #[arg(short, long)]
        account: Option<String>,
    },
    /// 予定を編集（指定した項目だけ変わる）  ID は list --ids で確認
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(short, long)]
        account: Option<String>,
    },
    /// 予定を削除  ID は list --ids で確認
    Delete {
        id: String,
        #[arg(short, long)]
        account: Option<String>,
    },
    /// TUI を起動
    Tui,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        None | Some(Cmd::Tui) => tui::run()?,
        Some(Cmd::Init { path }) => auth::init(&path)?,
        Some(Cmd::Login { name }) => auth::login(&name)?,
        Some(Cmd::Logout { name }) => auth::logout(&name)?,
        Some(Cmd::Accounts) => auth::accounts()?.iter().for_each(|a| println!("{a}")),
        Some(Cmd::List { days, account, all, ids }) => {
            let accounts = match account {
                Some(a) => vec![a],
                None => auth::accounts()?,
            };
            let (events, errors) = api::list_all(&accounts, Local::now().date_naive(), days);
            for e in events.iter().filter(|e| all || !e.declined()) {
                if ids { println!("{}  {}", e.line(), e.id) } else { println!("{}", e.line()) }
            }
            errors.iter().for_each(|e| eprintln!("エラー {e}"));
        }
        Some(Cmd::Add { title, start, end, account }) => {
            let p = Patch {
                summary: Some(title),
                start: Some(When::parse(&start, false)?),
                end: Some(When::parse(&end, true)?),
            };
            println!("追加しました: {}", api::save(&pick(account)?, None, &p)?.line());
        }
        Some(Cmd::Edit { id, title, start, end, account }) => {
            let p = Patch {
                summary: title,
                start: start.map(|s| When::parse(&s, false)).transpose()?,
                end: end.map(|s| When::parse(&s, true)).transpose()?,
            };
            println!("更新しました: {}", api::save(&pick(account)?, Some(&id), &p)?.line());
        }
        Some(Cmd::Delete { id, account }) => {
            api::delete(&pick(account)?, &id)?;
            println!("削除しました");
        }
    }
    Ok(())
}

/// -a 省略時、アカウントが1つだけならそれを使う
fn pick(account: Option<String>) -> Result<String> {
    if let Some(a) = account {
        return Ok(a);
    }
    match auth::accounts()?.as_slice() {
        [] => bail!("アカウントがありません。gcal login <アカウント名> で追加してください"),
        [only] => Ok(only.clone()),
        all => bail!("-a でアカウントを指定してください: {}", all.join(", ")),
    }
}
