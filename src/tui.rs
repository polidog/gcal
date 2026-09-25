use crate::api::{self, Event, Patch, When};
use crate::auth;
use anyhow::{Result, bail};
use chrono::{Duration, Local, NaiveDate};
use ratatui::crossterm::event::{self, Event as Term, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

const DAYS: i64 = 7;
const HELP: &str = " j/k:移動 h/l:前後の週 t:今日 Tab:アカウント a:追加 e:編集 d:削除 x:不参加も表示 r:再読込 o:ブラウザ q:終了";
const FORM_HELP: &str =
    " Tab/↑↓:項目移動 Enter:保存 Esc:やめる  日時は 2026-09-26 10:00 / 終日なら 2026-09-26";
const LABELS: [&str; 3] = ["タイトル", "開始", "終了"];

enum Mode {
    Normal,
    ConfirmDelete,
    Form(Form),
}

struct Form {
    account: String,
    id: Option<String>, // None = 追加
    fields: [String; 3],
    focus: usize,
}

struct App {
    accounts: Vec<String>,
    filter: usize, // 0 = 全アカウント
    from: NaiveDate,
    events: Vec<Event>,
    state: ListState,
    status: String,
    mode: Mode,
    show_declined: bool,
}

pub fn run() -> Result<()> {
    let accounts = auth::accounts()?;
    if accounts.is_empty() {
        bail!("アカウントがありません。gcal login <アカウント名> で追加してください");
    }
    let mut app = App {
        accounts,
        filter: 0,
        from: Local::now().date_naive(),
        events: vec![],
        state: ListState::default().with_selected(Some(0)),
        status: String::new(),
        mode: Mode::Normal,
        show_declined: false,
    };
    app.reload();
    let mut term = ratatui::init();
    let r = app.run(&mut term);
    ratatui::restore();
    r
}

impl App {
    // ponytail: 取得中は画面が止まる。気になったら別スレッド化
    fn reload(&mut self) {
        let (events, errors) = api::list_all(&self.accounts, self.from, DAYS);
        self.events = events;
        self.status = errors.join(" / ");
    }

    fn visible(&self) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| self.filter == 0 || e.account == self.accounts[self.filter - 1])
            .filter(|e| self.show_declined || !e.declined())
            .collect()
    }

    fn selected(&self) -> Option<&Event> {
        self.visible().get(self.state.selected()?).copied()
    }

    fn move_week(&mut self, from: NaiveDate) {
        self.from = from;
        self.state.select(Some(0));
        self.reload();
    }

    fn run(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        loop {
            term.draw(|f| self.draw(f))?;
            let Term::Key(k) = event::read()? else {
                continue;
            };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match &mut self.mode {
                Mode::Normal => {
                    if self.normal_key(k.code) {
                        return Ok(());
                    }
                }
                Mode::ConfirmDelete => {
                    self.mode = Mode::Normal;
                    if k.code == KeyCode::Char('y') {
                        self.delete();
                    } else {
                        self.status.clear();
                    }
                }
                Mode::Form(form) => match k.code {
                    KeyCode::Esc => {
                        self.mode = Mode::Normal;
                        self.status.clear();
                    }
                    KeyCode::Enter => self.submit(),
                    KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % 3,
                    KeyCode::BackTab | KeyCode::Up => form.focus = (form.focus + 2) % 3,
                    KeyCode::Backspace => _ = form.fields[form.focus].pop(),
                    KeyCode::Char(c) => form.fields[form.focus].push(c),
                    _ => {}
                },
            }
        }
    }

    /// true を返したら終了
    fn normal_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('j') | KeyCode::Down => self.state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.state.select_previous(),
            KeyCode::Tab => {
                self.filter = (self.filter + 1) % (self.accounts.len() + 1);
                self.state.select(Some(0));
            }
            KeyCode::Char('l') | KeyCode::Right => self.move_week(self.from + Duration::days(DAYS)),
            KeyCode::Char('h') | KeyCode::Left => self.move_week(self.from - Duration::days(DAYS)),
            KeyCode::Char('t') => self.move_week(Local::now().date_naive()),
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('x') => {
                self.show_declined = !self.show_declined;
                self.state.select(Some(0));
            }
            KeyCode::Char('o') => {
                if let Some(url) = self.selected().and_then(|e| e.html_link.as_deref()) {
                    auth::open_browser(url);
                }
            }
            KeyCode::Char('a') => self.open_add(),
            KeyCode::Char('e') => {
                if let Some(e) = self.selected() {
                    let fields = [e.summary.clone(), e.start.input(false), e.end.input(true)];
                    let (account, id) = (e.account.clone(), Some(e.id.clone()));
                    self.mode = Mode::Form(Form {
                        account,
                        id,
                        fields,
                        focus: 0,
                    });
                    self.status.clear();
                }
            }
            KeyCode::Char('d') => {
                if let Some(e) = self.selected() {
                    self.status = format!("「{}」を削除しますか？ y で削除", e.summary);
                    self.mode = Mode::ConfirmDelete;
                }
            }
            _ => {}
        }
        false
    }

    /// 追加先は表示中のアカウント。全アカウント表示で複数あるときは選んでもらう
    fn open_add(&mut self) {
        let account = match (self.filter, self.accounts.as_slice()) {
            (0, [only]) => only.clone(),
            (0, _) => {
                self.status = "Tab で追加先のアカウントを選んでから a を押してください".into();
                return;
            }
            (i, _) => self.accounts[i - 1].clone(),
        };
        // 選択中の予定の日付を初期値にする（時刻だけ打てばよいように）
        let day = self
            .selected()
            .map_or(self.from, |e| e.start.local().date_naive());
        let fields = [String::new(), format!("{day} "), format!("{day} ")];
        self.mode = Mode::Form(Form {
            account,
            id: None,
            fields,
            focus: 0,
        });
        self.status.clear();
    }

    fn submit(&mut self) {
        let Mode::Form(f) = &self.mode else { return };
        let r = (|| {
            let p = Patch {
                summary: Some(f.fields[0].clone()),
                start: Some(When::parse(f.fields[1].trim(), false)?),
                end: Some(When::parse(f.fields[2].trim(), true)?),
            };
            api::save(&f.account, f.id.as_deref(), &p)
        })();
        match r {
            Ok(_) => {
                self.mode = Mode::Normal;
                self.reload();
            }
            Err(e) => self.status = format!("{e:#}"), // フォームは開いたまま直してもらう
        }
    }

    fn delete(&mut self) {
        let Some(e) = self.selected() else { return };
        match api::delete(&e.account, &e.id) {
            Ok(()) => self.reload(),
            Err(err) => self.status = format!("{err:#}"),
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let [top, main, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(f.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(main);

        let who = if self.filter == 0 {
            "全アカウント"
        } else {
            &self.accounts[self.filter - 1]
        };
        let to = self.from + Duration::days(DAYS - 1);
        let declined = if self.show_declined {
            "  不参加も表示中"
        } else {
            ""
        };
        let title = format!(
            " gcal  {who}  {} 〜 {}{declined}",
            self.from.format("%m/%d"),
            to.format("%m/%d")
        );
        f.render_widget(Line::from(title).bold(), top);

        let items: Vec<String> = self.visible().iter().map(|e| e.line()).collect();
        let detail = self.selected().map(Event::detail).unwrap_or_default();
        let list = List::new(items)
            .block(Block::bordered().title("予定"))
            .highlight_style(Style::new().reversed())
            .highlight_symbol("> ");
        f.render_stateful_widget(list, left, &mut self.state);
        f.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title("詳細")),
            right,
        );

        let footer = match (&self.mode, self.status.is_empty()) {
            (_, false) => Line::from(self.status.as_str()).red(),
            (Mode::Form(_), true) => Line::from(FORM_HELP).dim(),
            _ => Line::from(HELP).dim(),
        };
        f.render_widget(footer, bottom);

        if let Mode::Form(form) = &self.mode {
            let [area] = Layout::vertical([Constraint::Length(5)])
                .flex(Flex::Center)
                .areas(f.area());
            let [area] = Layout::horizontal([Constraint::Length(64)])
                .flex(Flex::Center)
                .areas(area);
            let lines: Vec<Line> = LABELS
                .iter()
                .zip(&form.fields)
                .enumerate()
                .map(|(i, (label, value))| match i == form.focus {
                    true => Line::from(format!("{label:　<4} {value}▏")).bold(),
                    false => Line::from(format!("{label:　<4} {value}")),
                })
                .collect();
            let title = format!(
                " {} [{}] ",
                if form.id.is_some() {
                    "編集"
                } else {
                    "追加"
                },
                form.account
            );
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines).block(Block::bordered().title(title)),
                area,
            );
        }
    }
}
