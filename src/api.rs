use crate::auth;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ureq::{Agent, Body, http::Response};

const EVENTS: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub start: When,
    #[serde(default)]
    pub end: When,
    pub location: Option<String>,
    pub description: Option<String>,
    pub html_link: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(skip)]
    pub account: String,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default, rename = "self")]
    pub is_self: bool,
    pub response_status: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
/// 使わない側も null で送る（PATCH で時刻指定⇔終日を切り替えると古い側が残るため）
pub struct When {
    pub date_time: Option<DateTime<FixedOffset>>,
    pub date: Option<NaiveDate>,
}

impl When {
    pub fn local(&self) -> DateTime<Local> {
        match (self.date_time, self.date) {
            (Some(dt), _) => dt.with_timezone(&Local),
            (_, Some(d)) => midnight(d),
            _ => DateTime::UNIX_EPOCH.with_timezone(&Local),
        }
    }

    /// "2026-09-26 10:00" は時刻指定、"2026-09-26" は終日（end は当日を含む）
    pub fn parse(s: &str, is_end: bool) -> Result<When> {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
            let dt = dt.and_local_timezone(Local).earliest().context("存在しない時刻です")?;
            return Ok(When { date_time: Some(dt.fixed_offset()), date: None });
        }
        let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("日時の形式が不正: {s}（例: \"2026-09-26 10:00\" / 2026-09-26）"))?;
        Ok(When { date_time: None, date: Some(if is_end { d + Duration::days(1) } else { d }) })
    }

    /// parse の逆。編集フォームの初期値に使う
    pub fn input(&self, is_end: bool) -> String {
        match (self.date_time, self.date) {
            (Some(_), _) => self.local().format("%Y-%m-%d %H:%M").to_string(),
            (_, Some(d)) => (if is_end { d - Duration::days(1) } else { d }).to_string(),
            _ => String::new(),
        }
    }
}

impl Event {
    /// 自分が「不参加」と返事した予定
    pub fn declined(&self) -> bool {
        self.attendees.iter().any(|a| a.is_self && a.response_status.as_deref() == Some("declined"))
    }

    pub fn line(&self) -> String {
        let s = self.start.local();
        let wd = ["月", "火", "水", "木", "金", "土", "日"][s.weekday().num_days_from_monday() as usize];
        let time = match self.start.date_time {
            Some(_) => format!("{}-{}", s.format("%H:%M"), self.end.local().format("%H:%M")),
            None => "終日       ".into(),
        };
        let mark = if self.declined() { " (不参加)" } else { "" };
        format!("{}({wd}) {time} [{}] {}{mark}", s.format("%m/%d"), self.account, self.summary)
    }

    pub fn detail(&self) -> String {
        let mut s = format!("{}\n\n{}\n{}", self.summary, self.line(), self.account);
        for v in [&self.location, &self.description].into_iter().flatten() {
            s += &format!("\n\n{v}");
        }
        s
    }
}

pub fn midnight(d: NaiveDate) -> DateTime<Local> {
    d.and_hms_opt(0, 0, 0).unwrap().and_local_timezone(Local).earliest().expect("深夜0時が存在しないタイムゾーン")
}

pub fn agent() -> Agent {
    Agent::config_builder().http_status_as_error(false).build().into()
}

/// エラー時は Google が返す本文（API 未有効化などの理由）をそのまま見せる
pub fn json<T: DeserializeOwned>(res: Response<Body>) -> Result<T> {
    Ok(check(res)?.body_mut().read_json()?)
}

fn check(mut res: Response<Body>) -> Result<Response<Body>> {
    if !res.status().is_success() {
        bail!("{} {}", res.status(), res.body_mut().read_to_string()?);
    }
    Ok(res)
}

fn list(account: &str, from: NaiveDate, days: i64) -> Result<Vec<Event>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Page {
        #[serde(default)]
        items: Vec<Event>,
        next_page_token: Option<String>,
    }
    let token = auth::access_token(account)?;
    let (min, max) = (midnight(from).to_rfc3339(), midnight(from + Duration::days(days)).to_rfc3339());
    let mut out = vec![];
    let mut page_token = None;
    loop {
        let mut req = agent()
            .get(EVENTS)
            .header("Authorization", format!("Bearer {token}"))
            .query("timeMin", &min)
            .query("timeMax", &max)
            .query("singleEvents", "true")
            .query("orderBy", "startTime")
            .query("maxResults", "250");
        if let Some(p) = &page_token {
            req = req.query("pageToken", p);
        }
        let mut page: Page = json(req.call()?)?;
        for e in &mut page.items {
            e.account = account.into();
        }
        out.append(&mut page.items);
        page_token = page.next_page_token;
        if page_token.is_none() {
            return Ok(out);
        }
    }
}

/// アカウントごとの失敗は errors に積み、取れた分だけ返す
pub fn list_all(accounts: &[String], from: NaiveDate, days: i64) -> (Vec<Event>, Vec<String>) {
    let (mut events, mut errors) = (vec![], vec![]);
    // ponytail: アカウントを順番に取得。数が増えて遅ければ thread::scope で並列化
    for a in accounts {
        match list(a, from, days) {
            Ok(v) => events.extend(v),
            Err(e) => errors.push(format!("{a}: {e:#}")),
        }
    }
    events.sort_by_key(|e| e.start.local());
    (events, errors)
}

/// 追加・更新の本文。None の項目は送らない（PATCH で変更しない）
#[derive(Serialize, Default)]
pub struct Patch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<When>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<When>,
}

/// id なしなら新規作成、ありなら更新
pub fn save(account: &str, id: Option<&str>, p: &Patch) -> Result<Event> {
    let auth = format!("Bearer {}", auth::access_token(account)?);
    let res = match id {
        None => agent().post(EVENTS).header("Authorization", auth).send_json(p)?,
        Some(id) => agent().patch(format!("{EVENTS}/{id}")).header("Authorization", auth).send_json(p)?,
    };
    let mut e: Event = json(res)?;
    e.account = account.into();
    Ok(e)
}

pub fn delete(account: &str, id: &str) -> Result<()> {
    let auth = format!("Bearer {}", auth::access_token(account)?);
    check(agent().delete(format!("{EVENTS}/{id}")).header("Authorization", auth).call()?)?;
    Ok(())
}

#[test]
fn when_roundtrip() {
    for (s, is_end) in [("2026-09-26 10:00", false), ("2026-09-26", false), ("2026-09-26", true)] {
        assert_eq!(When::parse(s, is_end).unwrap().input(is_end), s);
    }
    assert_eq!(When::parse("2026-09-26", true).unwrap().date.unwrap().to_string(), "2026-09-27");
    assert!(When::parse("09/26", false).is_err());
}

#[test]
fn declined() {
    let e: Event = serde_json::from_str(
        r#"{"attendees":[{"email":"a","responseStatus":"declined"},{"self":true,"responseStatus":"accepted"}]}"#,
    )
    .unwrap();
    assert!(!e.declined());
    let e: Event = serde_json::from_str(r#"{"attendees":[{"self":true,"responseStatus":"declined"}]}"#).unwrap();
    assert!(e.declined());
    assert!(!serde_json::from_str::<Event>("{}").unwrap().declined());
}
