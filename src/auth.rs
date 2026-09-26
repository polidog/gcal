use crate::api::{agent, json};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::{env, fs};

const SCOPE: &str = "https://www.googleapis.com/auth/calendar.events";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Serialize, Deserialize)]
struct Client {
    client_id: String,
    client_secret: String,
}

#[derive(Serialize, Deserialize)]
struct Token {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

fn dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .expect("HOME が設定されていません")
        .join("gcal")
}

fn account_path(name: &str) -> PathBuf {
    dir().join("accounts").join(format!("{name}.json"))
}

fn write_private(path: &Path, data: &str) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let mut o = fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(data.as_bytes())?;
    Ok(())
}

/// Google Cloud Console で作った「デスクトップアプリ」の client_secret.json を取り込む
pub fn init(path: &Path) -> Result<()> {
    #[derive(Deserialize)]
    struct Secret {
        installed: Client,
    }
    let s: Secret = serde_json::from_str(&fs::read_to_string(path)?)
        .context("デスクトップアプリ用の client_secret.json ではありません")?;
    write_private(
        &dir().join("client.json"),
        &serde_json::to_string(&s.installed)?,
    )?;
    println!("登録しました。次は gcal login <アカウント名>");
    Ok(())
}

fn client() -> Result<Client> {
    let s = fs::read_to_string(dir().join("client.json"))
        .context("先に gcal init <client_secret.json> を実行してください")?;
    Ok(serde_json::from_str(&s)?)
}

pub fn accounts() -> Result<Vec<String>> {
    let Ok(rd) = fs::read_dir(dir().join("accounts")) else {
        return Ok(vec![]);
    };
    let mut v: Vec<String> = rd
        .filter_map(|e| Some(e.ok()?.path().file_stem()?.to_str()?.to_string()))
        .collect();
    v.sort();
    Ok(v)
}

pub fn logout(name: &str) -> Result<()> {
    fs::remove_file(account_path(name)).with_context(|| format!("アカウント {name} はありません"))
}

/// ループバックで認可コードを受け取る。ブラウザで許可するだけで完了する
pub fn login(name: &str) -> Result<()> {
    let c = client()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let state = format!(
        "{:x}{:x}",
        RandomState::new().hash_one(0),
        RandomState::new().hash_one(1)
    );
    let url = format!(
        "{AUTH_URL}?response_type=code&access_type=offline&prompt=consent&client_id={}&redirect_uri={}&scope={}&state={state}",
        enc(&c.client_id),
        enc(&redirect),
        enc(SCOPE)
    );
    eprintln!("ブラウザで許可してください（開かない場合は下の URL を開く）:\n{url}");
    open_browser(&url);

    // ブラウザの先読み接続などは読み捨てて、code か error を含むリクエストを待つ
    let (query, mut stream) = loop {
        let (stream, _) = listener.accept()?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        if let Some((_, q)) = line
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.split_once('?'))
            && (q.contains("code=") || q.contains("error="))
        {
            break (q.to_string(), stream);
        }
    };
    let code = param(&query, "code").filter(|_| param(&query, "state").as_deref() == Some(&state));
    let msg = if code.is_some() {
        "gcal: ログインしました。このタブは閉じて大丈夫です"
    } else {
        "gcal: ログインに失敗しました"
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<h1>{msg}</h1>"
    );
    let Some(code) = code else {
        bail!("認可に失敗しました: {query}")
    };

    let r: TokenResp = json(agent().post(TOKEN_URL).send_form([
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("client_id", &c.client_id),
        ("client_secret", &c.client_secret),
        ("redirect_uri", &redirect),
    ])?)?;
    let t = Token {
        access_token: r.access_token,
        refresh_token: r
            .refresh_token
            .context("refresh_token が返りませんでした")?,
        expires_at: Utc::now().timestamp() + r.expires_in - 60,
    };
    write_private(&account_path(name), &serde_json::to_string(&t)?)?;
    println!("アカウント {name} を追加しました");
    Ok(())
}

/// 期限切れなら refresh_token で更新してから返す
pub fn access_token(name: &str) -> Result<String> {
    let path = account_path(name);
    let s = fs::read_to_string(&path).with_context(|| format!("アカウント {name} はありません"))?;
    let mut t: Token = serde_json::from_str(&s)?;
    if Utc::now().timestamp() < t.expires_at {
        return Ok(t.access_token);
    }
    let c = client()?;
    let r: TokenResp = json(agent().post(TOKEN_URL).send_form([
        ("grant_type", "refresh_token"),
        ("refresh_token", &t.refresh_token),
        ("client_id", &c.client_id),
        ("client_secret", &c.client_secret),
    ])?)
    .with_context(|| format!("トークン更新に失敗。gcal login {name} で再ログインしてください"))?;
    t.access_token = r.access_token;
    t.expires_at = Utc::now().timestamp() + r.expires_in - 60;
    write_private(&path, &serde_json::to_string(&t)?)?;
    Ok(t.access_token)
}

pub fn open_browser(url: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
        .map(dec)
}

fn enc(s: &str) -> String {
    use std::fmt::Write;
    s.bytes()
        .fold(String::with_capacity(s.len()), |mut out, b| {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    out.push(b as char)
                }
                _ => _ = write!(out, "%{b:02X}"),
            }
            out
        })
}

fn dec(s: &str) -> String {
    let mut out = Vec::new();
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        out.push(match b {
            b'+' => b' ',
            b'%' => {
                let h: String = it.by_ref().take(2).map(char::from).collect();
                u8::from_str_radix(&h, 16).unwrap_or(b'?')
            }
            _ => b,
        });
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn query_roundtrip() {
    let q = format!("state=abc&code={}&scope=x", enc("4/0Ab+c/d e"));
    assert_eq!(param(&q, "code").as_deref(), Some("4/0Ab+c/d e"));
    assert_eq!(param(&q, "state").as_deref(), Some("abc"));
    assert_eq!(param(&q, "cod"), None);
}
