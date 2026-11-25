//! In-game title accounts, using the launcher store when connected.

use super::{
    avatars,
    scroll::Scroll,
    standalone_accounts,
    textfield::TextField,
    ui::{self, Ui},
};
use crate::native_ui::{Canvas, font, icons, theme};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const ROW: f32 = 47.0;
const CHIP_HEIGHT: f32 = 36.0;
const PANEL_BG: u32 = 0xFC08090B;
const REFRESH_MS: f64 = 1500.0;

pub(super) fn tr(key: &str) -> String {
    horizon_client::lang::get_or(&format!("native.accounts.{key}"), "")
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    name: String,
    #[serde(rename = "type")]
    provider: String,
    uuid: Option<String>,
    current: bool,
    sign_in_required: bool,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    error: String,
    accounts: Option<Vec<Account>>,
    #[serde(rename = "addedAccountId")]
    added_account_id: Option<String>,
    session: Option<SessionGrant>,
    code: Option<String>,
    url: Option<String>,
}

#[derive(Deserialize, Clone, PartialEq, Eq)]
pub struct SessionGrant {
    pub id: String,
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub online: bool,
}

enum Action {
    List,
    AddOffline(String),
    NativeLogin(String, String),
    MicrosoftStart,
    MicrosoftStatus,
    MicrosoftCancel,
    Switch(String),
    Applied(String),
}

impl Action {
    fn request(&self) -> Value {
        match self {
            Self::List => json!({"action":"list"}),
            Self::AddOffline(name) => json!({"action":"add_offline","name":name}),
            Self::NativeLogin(login, password) => {
                json!({"action":"native_login","login":login,"password":password})
            }
            Self::MicrosoftStart => json!({"action":"microsoft_start"}),
            Self::MicrosoftStatus => json!({"action":"microsoft_status"}),
            Self::MicrosoftCancel => json!({"action":"microsoft_cancel"}),
            Self::Switch(id) => json!({"action":"switch","account_id":id}),
            Self::Applied(id) => json!({"action":"applied","account_id":id}),
        }
    }
}

struct Pending {
    action: Action,
    result: Receiver<Option<Reply>>,
}

#[derive(PartialEq)]
enum Panel {
    Closed,
    Dropdown,
    Add,
}

pub struct TitleAccounts {
    bridge: Option<(SocketAddr, String)>,
    standalone: Arc<Mutex<standalone_accounts::Accounts>>,
    pending: Option<Pending>,
    queued: Option<Action>,
    refresh_at: f64,
    accounts: Vec<Account>,
    selected: Option<Account>,
    username: TextField,
    login: TextField,
    password: TextField,
    scroll: Scroll,
    status: String,
    panel: Panel,
    offline: bool,
    native_form: bool,
    microsoft_form: bool,
    microsoft_code: String,
    microsoft_url: String,
    microsoft_poll: f64,
    dropdown_motion: f32,
    modal_motion: f32,
    reduced_motion: bool,
    loaded: bool,
    failed: bool,
}

impl Default for TitleAccounts {
    fn default() -> Self {
        let mut username = TextField::new("title:accounts:username");
        username.max_length = 16;
        let mut login = TextField::new("title:accounts:login");
        login.max_length = 160;
        let mut password = TextField::new("title:accounts:password");
        password.masked = true;
        password.max_length = 256;
        let port = std::env::var("NATIVE_TITLE_ACCOUNTS_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|port| *port != 0);
        let key = std::env::var("NATIVE_TITLE_ACCOUNTS_KEY")
            .ok()
            .filter(|key| !key.is_empty());
        Self {
            bridge: port
                .zip(key)
                .map(|(port, key)| ((Ipv4Addr::LOCALHOST, port).into(), key)),
            standalone: Arc::default(),
            pending: None,
            queued: None,
            refresh_at: 0.0,
            accounts: Vec::new(),
            selected: None,
            username,
            login,
            password,
            scroll: Scroll::new("title:accounts"),
            status: String::new(),
            panel: Panel::Closed,
            offline: false,
            native_form: false,
            microsoft_form: false,
            microsoft_code: String::new(),
            microsoft_url: String::new(),
            microsoft_poll: 0.0,
            dropdown_motion: 0.0,
            modal_motion: 0.0,
            reduced_motion: std::env::var_os("NATIVE_REDUCED_MOTION").is_some_and(|v| v == "1"),
            loaded: false,
            failed: false,
        }
    }
}

impl TitleAccounts {
    pub fn is_open(&self) -> bool {
        self.panel != Panel::Closed
    }

    pub(super) fn applied(&mut self, id: String) {
        self.send(Action::Applied(id));
    }

    pub(super) fn apply_failed(&mut self) {
        self.status = tr("switch_failed");
    }

    pub(super) fn draw(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        right: f32,
        top: f32,
        player: &str,
        entrance: f32,
    ) -> Option<SessionGrant> {
        let editing = self.panel == Panel::Add && (self.offline || self.native_form);
        if let Some(grant) = self.check_reply(ui.now) {
            return Some(grant);
        }
        if editing && self.panel != Panel::Add {
            ui.focus = None;
        }
        let interactive = ui.interactive;
        if interactive && self.is_open() && ui.key(ui::KEY_ESCAPE) && !self.switching() {
            self.back(ui, false);
        }
        if self.panel == Panel::Add
            && self.microsoft_form
            && !self.microsoft_code.is_empty()
            && self.pending.is_none()
            && ui.now >= self.microsoft_poll
        {
            self.microsoft_poll = ui.now + REFRESH_MS;
            self.send(Action::MicrosoftStatus);
        } else if self.is_open() && self.pending.is_none() && ui.now >= self.refresh_at {
            self.send(Action::List);
        }

        ui.interactive = interactive && self.panel != Panel::Add;
        let chip = (c.text_width(font::SEMIBOLD, 12.5, player) + 65.0).clamp(118.0, 172.0);
        c.push_alpha(entrance);
        self.draw_chip(ui, c, right - chip, top, chip, player);
        c.pop_alpha();
        self.dropdown_motion =
            self.animate(self.dropdown_motion, self.panel == Panel::Dropdown, ui.dt);
        self.modal_motion = self.animate(self.modal_motion, self.panel == Panel::Add, ui.dt);

        ui.interactive = interactive && self.panel == Panel::Dropdown;
        if self.dropdown_motion > 0.0 {
            self.draw_dropdown(ui, c, right, top + CHIP_HEIGHT + 7.0, chip);
        }
        ui.interactive = interactive && self.panel == Panel::Add;
        if self.modal_motion > 0.0 {
            self.draw_add_panel(ui, c);
        }
        ui.interactive = interactive;
        None
    }

    fn animate(&self, value: f32, visible: bool, dt: f32) -> f32 {
        let target = if visible { 1.0 } else { 0.0 };
        if self.reduced_motion {
            return target;
        }
        let next = value + (target - value) * (1.0 - (-23.0 * dt).exp());
        if (next - target).abs() < 0.01 {
            target
        } else {
            next
        }
    }

    fn switching(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| matches!(p.action, Action::Switch(_) | Action::Applied(_)))
            || matches!(self.queued, Some(Action::Switch(_)))
    }

    fn back(&mut self, ui: &mut Ui, close: bool) {
        self.queued = None;
        self.status.clear();
        match self.panel {
            Panel::Add => {
                if self.microsoft_form {
                    self.send(Action::MicrosoftCancel);
                }
                if close || !(self.offline || self.native_form || self.microsoft_form) {
                    self.panel = Panel::Dropdown;
                }
                self.clear_form();
            }
            Panel::Dropdown => {
                if self.selected.is_none() || close {
                    self.panel = Panel::Closed;
                }
                self.selected = None;
            }
            Panel::Closed => {}
        }
        ui.focus = None;
    }

    fn draw_chip(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, player: &str) {
        let over = ui.hover(c, x, y, w, CHIP_HEIGHT);
        let hover = ui.anim_on("title:accounts#hover", over || self.is_open(), 16.0);
        c.round(
            x,
            y,
            w,
            CHIP_HEIGHT,
            10.0,
            theme::mix(0xE9090A0C, 0xFA121316, hover),
        );
        c.outline(
            x,
            y,
            w,
            CHIP_HEIGHT,
            10.0,
            1.0,
            theme::mix(0x20FFFFFF, 0x36FFFFFF, hover),
        );
        avatars::draw(
            c,
            avatars::own(player).as_deref(),
            x + 7.0,
            y + 7.0,
            22.0,
            5.0,
        );
        let name = c.ellipsize(font::SEMIBOLD, 12.5, player, w - 61.0);
        let text_y = y + (CHIP_HEIGHT - c.line_height(font::SEMIBOLD, 12.5)) / 2.0;
        c.text(
            font::SEMIBOLD,
            12.5,
            &name,
            x + 37.0,
            text_y,
            theme::TEXT_STRONG,
        );
        let chevron = if self.panel == Panel::Dropdown {
            theme::I_CHEVRON_UP
        } else {
            theme::I_CHEVRON_DOWN
        };
        c.icon(
            chevron,
            12.0,
            x + w - 14.0,
            y + CHIP_HEIGHT / 2.0,
            theme::TEXT_SECONDARY,
        );
        if over && !self.is_open() {
            ui.tip(&tr("title"));
        }
        if ui.clicked(c, "title:accounts", x, y, w, CHIP_HEIGHT) && !self.switching() {
            if self.panel == Panel::Dropdown {
                self.back(ui, true);
            } else {
                self.panel = Panel::Dropdown;
                self.refresh_at = 0.0;
                self.status.clear();
                self.failed = false;
            }
        }
    }

    fn draw_dropdown(&mut self, ui: &mut Ui, c: &mut Canvas, right: f32, top: f32, chip: f32) {
        let w = 290.0f32.min(right - 8.0);
        let x = right - w;
        let y = top - 8.0 * (1.0 - self.dropdown_motion);
        let message = self.message();
        let rows = (self.accounts.len() as f32 * ROW)
            .clamp(ROW, 4.0 * ROW)
            .min((c.height() - top - 106.0).max(ROW));
        let h = 82.0 + rows + if message.is_empty() { 0.0 } else { 20.0 };
        let on_chip = ui.hover(c, right - chip, top - CHIP_HEIGHT - 7.0, chip, CHIP_HEIGHT);
        if ui.interactive && ui.pressed && !self.switching() && !on_chip && !ui.hover(c, x, y, w, h)
        {
            self.back(ui, true);
            ui.interactive = false;
        }
        c.push_alpha(self.dropdown_motion);
        c.shadow(x, y + 4.0, w, h, 13.0, 21.0, 0xA4000000);
        c.round(x, y, w, h, 13.0, PANEL_BG);
        c.outline(x, y, w, h, 13.0, 1.0, theme::HAIRLINE_STRONG);
        c.text(
            font::SEMIBOLD,
            12.0,
            &tr("switch_account"),
            x + 14.0,
            y + 11.0,
            theme::TEXT_SECONDARY,
        );
        let (left, inner, list_top) = (x + 8.0, w - 16.0, y + 34.0);
        let delta = self.scroll.begin(ui, c, left, list_top, inner, rows);
        for (i, account) in self.accounts.iter().enumerate() {
            let ry = list_top + i as f32 * ROW - delta;
            if ry + ROW > list_top
                && ry < list_top + rows
                && self.draw_row(ui, c, account, left, ry, inner - 7.0)
                && !self.switching()
            {
                self.selected = Some(account.clone());
                self.status.clear();
            }
        }
        if self.accounts.is_empty() {
            let empty = if self.failed {
                "load_failed"
            } else if self.loaded {
                "empty"
            } else {
                "loading"
            };
            let note = c.ellipsize(font::REGULAR, 12.0, &tr(empty), inner - 16.0);
            c.text(
                font::REGULAR,
                12.0,
                &note,
                left + 6.0,
                list_top + 12.0,
                theme::TEXT_SECONDARY,
            );
        }
        self.scroll.end(ui, c, self.accounts.len() as f32 * ROW);
        let bar = list_top + rows + 5.0;
        c.fill(left + 4.0, bar - 3.0, inner - 8.0, 1.0, theme::HAIRLINE);
        if let Some(account) = self.selected.clone() {
            let confirm = if account.sign_in_required {
                "sign_in"
            } else {
                "switch"
            };
            if ui.button(
                c,
                "title:accounts:back",
                left,
                bar,
                68.0,
                34.0,
                &tr("back"),
                0,
                ui::BTN_GHOST,
            ) && !self.switching()
            {
                self.back(ui, false);
            } else if ui.button(
                c,
                "title:accounts:confirm",
                left + 75.0,
                bar,
                inner - 75.0,
                34.0,
                &tr(confirm),
                0,
                ui::BTN_PRIMARY,
            ) || (ui.interactive && (ui.key(ui::KEY_ENTER) || ui.key(ui::KEY_KP_ENTER)))
            {
                if account.sign_in_required && account.provider == "microsoft" {
                    self.open_microsoft();
                } else if account.sign_in_required {
                    self.open_native(ui);
                } else {
                    self.send(Action::Switch(account.id));
                }
            }
        } else if add_button(ui, c, left, bar, inner) {
            self.panel = Panel::Add;
            self.offline = false;
            self.status.clear();
        }
        draw_status(c, &message, left + 5.0, bar + 38.0, inner - 10.0);
        c.pop_alpha();
    }

    fn draw_add_panel(&mut self, ui: &mut Ui, c: &mut Canvas) {
        let w = 420.0f32.min(c.width() - 24.0);
        let available = c.height();
        let step = ((available - 112.0) / 3.0).clamp(42.0, 62.0);
        let h = if self.offline {
            208.0
        } else if self.native_form {
            266.0
        } else if self.microsoft_form {
            224.0
        } else {
            107.0 + step * 3.0
        };
        let (x, y) = (
            (c.width() - w) / 2.0,
            (available - h) / 2.0 + 12.0 * (1.0 - self.modal_motion),
        );
        c.fill(
            0.0,
            0.0,
            c.width(),
            available,
            theme::alpha(0xAD000000, self.modal_motion),
        );
        c.push_alpha(self.modal_motion);
        super::menu::panel(c, x, y, w, h);
        c.round(x + 18.0, y + 18.0, 36.0, 36.0, 10.0, theme::SUBTLE);
        let storage = if self.bridge.is_some() {
            "shared"
        } else {
            "local"
        };
        let (heading, description) = if self.offline {
            c.icon(
                theme::I_WIFI_OFF,
                17.0,
                x + 36.0,
                y + 36.0,
                theme::TEXT_STRONG,
            );
            ("offline_account", "offline_help")
        } else if self.native_form {
            provider_mark(c, "native", x + 36.0, y + 36.0, 17.0);
            ("native_title", storage)
        } else if self.microsoft_form {
            provider_mark(c, "microsoft", x + 36.0, y + 36.0, 17.0);
            ("microsoft_title", "microsoft_code_help")
        } else {
            c.icon(
                theme::I_USER_PLUS,
                17.0,
                x + 36.0,
                y + 36.0,
                theme::TEXT_STRONG,
            );
            ("add_account", storage)
        };
        c.text(
            font::SEMIBOLD,
            19.0,
            &tr(heading),
            x + 65.0,
            y + 16.0,
            theme::TEXT_STRONG,
        );
        let description = c.ellipsize(font::REGULAR, 11.5, &tr(description), w - 110.0);
        c.text(
            font::REGULAR,
            11.5,
            &description,
            x + 65.0,
            y + 43.0,
            theme::TEXT_SECONDARY,
        );
        if ui.button(
            c,
            "title:accounts:add:close",
            x + w - 42.0,
            y + 17.0,
            26.0,
            26.0,
            "",
            theme::I_X,
            ui::BTN_GHOST,
        ) || (ui.interactive && ui.pressed && !ui.hover(c, x, y, w, h))
        {
            self.back(ui, true);
            ui.interactive = false;
        }

        let (left, inner, start) = (x + 18.0, w - 36.0, y + 73.0);
        if self.native_form {
            self.draw_native_form(ui, c, left, start, inner);
        } else if self.microsoft_form {
            self.draw_microsoft_form(ui, c, left, start, inner);
        } else if self.offline {
            c.text(
                font::SEMIBOLD,
                11.5,
                &tr("username"),
                left,
                start,
                theme::TEXT_SECONDARY,
            );
            let before = self.username.value();
            let entered = ui.interactive
                && self.username.draw(
                    ui,
                    c,
                    left,
                    start + 20.0,
                    inner,
                    36.0,
                    &tr("username_example"),
                    13.0,
                );
            let value = self.username.value();
            if value != before {
                self.status.clear();
            }
            if !ui.interactive {
                c.round(left, start + 20.0, inner, 36.0, 10.0, theme::SUBTLE);
                c.text(
                    font::REGULAR,
                    13.0,
                    &value,
                    left + 12.0,
                    start + 30.0,
                    theme::TEXT_STRONG,
                );
            }
            let bar = start + 72.0;
            if ui.button(
                c,
                "title:accounts:offline:back",
                left,
                bar,
                76.0,
                34.0,
                &tr("back"),
                theme::I_ARROW_LEFT,
                ui::BTN_GHOST,
            ) {
                self.back(ui, false);
            } else if ui.button(
                c,
                "title:accounts:save-offline",
                left + 84.0,
                bar,
                inner - 84.0,
                34.0,
                &tr("save"),
                0,
                ui::BTN_PRIMARY,
            ) || entered
            {
                let name = value.trim();
                if (3..=16).contains(&name.len())
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    self.send(Action::AddOffline(name.to_owned()));
                } else {
                    self.status = tr("bad_name");
                    self.username.focus(ui);
                }
            }
            draw_status(c, &self.message(), left, start + 114.0, inner);
        } else {
            for (i, (provider, label, help)) in [
                ("microsoft", "microsoft", "microsoft_help"),
                ("native", "native", "native_help"),
                ("offline", "offline_type", "offline_provider_help"),
            ]
            .into_iter()
            .enumerate()
            {
                if provider_button(
                    ui,
                    c,
                    provider,
                    left,
                    start + step * i as f32,
                    inner,
                    step - 6.0,
                    &tr(label),
                    &tr(help),
                ) {
                    if provider == "offline" {
                        self.offline = true;
                        self.status.clear();
                        self.username.focus(ui);
                    } else if provider == "native" {
                        self.open_native(ui);
                    } else {
                        self.open_microsoft();
                    }
                }
            }
            draw_status(c, &self.message(), left, start + step * 3.0 + 5.0, inner);
        }
        c.pop_alpha();
    }

    fn open_microsoft(&mut self) {
        self.panel = Panel::Add;
        self.microsoft_form = true;
        self.status.clear();
        self.send(Action::MicrosoftStart);
    }

    fn open_native(&mut self, ui: &mut Ui) {
        self.panel = Panel::Add;
        self.native_form = true;
        self.status.clear();
        if let Some(account) = &self.selected {
            self.login.set(&account.name);
        }
        self.login.focus(ui);
    }

    fn clear_form(&mut self) {
        self.offline = false;
        self.native_form = false;
        self.microsoft_form = false;
        self.microsoft_code.clear();
        self.microsoft_url.clear();
        self.login.clear();
        self.password.clear();
    }

    fn draw_native_form(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) {
        c.text(
            font::SEMIBOLD,
            11.5,
            &tr("login"),
            x,
            y,
            theme::TEXT_SECONDARY,
        );
        let next = ui.interactive && self.login.draw(ui, c, x, y + 18.0, w, 34.0, "", 13.0);
        c.text(
            font::SEMIBOLD,
            11.5,
            &tr("password"),
            x,
            y + 59.0,
            theme::TEXT_SECONDARY,
        );
        let entered = ui.interactive && self.password.draw(ui, c, x, y + 77.0, w, 34.0, "", 13.0);
        if next || (ui.interactive && ui.key(ui::KEY_TAB)) {
            if !next && self.password.focused(ui) {
                self.login.focus(ui);
            } else {
                self.password.focus(ui);
            }
        }
        if ui.button(
            c,
            "title:accounts:native:back",
            x,
            y + 124.0,
            76.0,
            34.0,
            &tr("back"),
            theme::I_ARROW_LEFT,
            ui::BTN_GHOST,
        ) {
            self.back(ui, false);
        } else if ui.button(
            c,
            "title:accounts:native:submit",
            x + 84.0,
            y + 124.0,
            w - 84.0,
            34.0,
            &tr("sign_in_submit"),
            0,
            ui::BTN_PRIMARY,
        ) || entered
        {
            let login = self.login.value().trim().to_owned();
            let password = self.password.value();
            if login.is_empty() || password.is_empty() {
                self.status = tr("credentials");
            } else {
                self.send(Action::NativeLogin(login, password));
                self.password.clear();
            }
        }
        draw_status(c, &self.message(), x, y + 165.0, w);
    }

    fn draw_microsoft_form(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) {
        let has_code = !self.microsoft_code.is_empty();
        let code = if has_code {
            &self.microsoft_code
        } else {
            "—"
        };
        c.round(x, y, w, 43.0, 10.0, theme::SUBTLE);
        let tw = c.text_width(font::BOLD, 22.0, code);
        c.text(
            font::BOLD,
            22.0,
            code,
            x + (w - tw) / 2.0,
            y + 7.0,
            theme::TEXT_STRONG,
        );
        draw_status(c, &self.microsoft_url, x, y + 48.0, w);
        if ui.button(
            c,
            "title:accounts:ms:back",
            x,
            y + 71.0,
            76.0,
            34.0,
            &tr("back"),
            theme::I_ARROW_LEFT,
            ui::BTN_GHOST,
        ) {
            self.back(ui, false);
        } else if has_code {
            if ui.button(
                c,
                "title:accounts:ms:open",
                x + 84.0,
                y + 71.0,
                w - 84.0,
                34.0,
                &tr("copy_code"),
                0,
                ui::BTN_PRIMARY,
            ) {
                ui.clip_out = Some(self.microsoft_code.clone());
                let url = self.microsoft_url.clone();
                // The authorization endpoint supplies an HTTPS Microsoft URL.
                if url.starts_with("https://www.microsoft.com/")
                    || url.starts_with("https://microsoft.com/")
                {
                    super::common::open_url(&url);
                }
            }
        } else if ui.button(
            c,
            "title:accounts:ms:start",
            x + 84.0,
            y + 71.0,
            w - 84.0,
            34.0,
            &tr("try_again"),
            0,
            ui::BTN_PRIMARY,
        ) {
            self.send(Action::MicrosoftStart);
        }
        let status = if has_code && self.status.is_empty() {
            tr("waiting_ms")
        } else {
            self.message()
        };
        draw_status(c, &status, x, y + 113.0, w);
    }

    fn draw_row(
        &self,
        ui: &mut Ui,
        c: &mut Canvas,
        account: &Account,
        x: f32,
        y: f32,
        w: f32,
    ) -> bool {
        let picked = self.selected.as_ref().is_some_and(|a| a.id == account.id);
        let key = format!("title:accounts:row:{}", account.id);
        let over = ui.hover(c, x, y, w, ROW - 3.0);
        let hover = ui.anim_on(&format!("{key}#h"), over, 18.0);
        let base = if picked {
            0x23FFFFFF
        } else if account.current {
            0x0DFFFFFF
        } else {
            0
        };
        c.round(x, y, w, ROW - 3.0, 8.0, theme::mix(base, 0x1AFFFFFF, hover));
        if picked {
            c.outline(x, y, w, ROW - 3.0, 8.0, 1.0, theme::HAIRLINE_STRONG);
        }
        let skin = match account.provider.as_str() {
            "microsoft" => account
                .uuid
                .as_ref()
                .map(|uuid| format!("mj:{}", uuid.replace('-', "").to_ascii_lowercase())),
            "native" => horizon_client::native_skins::directory()
                .entry(&account.name)
                .and_then(|entry| entry.skin.clone()),
            _ => None,
        };
        avatars::draw(c, skin.as_deref(), x + 7.0, y + 8.0, 28.0, 6.0);
        let name = c.ellipsize(font::SEMIBOLD, 12.5, &account.name, w - 77.0);
        c.text(
            font::SEMIBOLD,
            12.5,
            &name,
            x + 45.0,
            y + 5.0,
            theme::TEXT_STRONG,
        );
        let mut detail = match account.provider.as_str() {
            "microsoft" => tr("microsoft"),
            "native" => tr("native"),
            _ => tr("offline_type"),
        };
        if account.sign_in_required {
            detail.push_str(" · ");
            detail.push_str(&tr("sign_in"));
        }
        provider_mark(c, &account.provider, x + 50.0, y + 31.0, 9.0);
        let detail = c.ellipsize(font::REGULAR, 10.5, &detail, w - 85.0);
        c.text(
            font::REGULAR,
            10.5,
            &detail,
            x + 60.0,
            y + 24.0,
            theme::TEXT_SECONDARY,
        );
        let (icon, tint) = if account.current {
            (theme::I_CHECK, theme::TEXT_STRONG)
        } else {
            (theme::I_CHEVRON_RIGHT, theme::TEXT_MUTED)
        };
        c.icon(icon, 13.0, x + w - 14.0, y + 22.0, tint);
        if account.current && over {
            ui.tip(&tr("current"));
        }
        (!account.current || account.sign_in_required) && ui.clicked(c, &key, x, y, w, ROW - 3.0)
    }

    fn message(&self) -> String {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| !matches!(p.action, Action::List))
            || self.queued.is_some()
        {
            tr("working")
        } else {
            self.status.clone()
        }
    }

    fn send(&mut self, action: Action) {
        if let Some(pending) = &self.pending {
            if (matches!(pending.action, Action::List | Action::MicrosoftStatus)
                || matches!(action, Action::MicrosoftCancel))
                && !matches!(action, Action::List)
            {
                self.queued = Some(action);
            }
            return;
        }
        let mut request = action.request();
        let bridge = self.bridge.clone();
        let standalone = self.standalone.clone();
        // Passwords are needed only in the request, never in the in-flight UI state.
        let action = match action {
            Action::NativeLogin(_, _) => Action::NativeLogin(String::new(), String::new()),
            action => action,
        };
        let (tx, result) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let reply = if let Some((address, key)) = bridge {
                request["key"] = key.into();
                exchange(address, request)
            } else {
                standalone.lock().ok().and_then(|mut accounts| {
                    serde_json::from_value(accounts.exchange(&request)).ok()
                })
            };
            let _ = tx.send(reply);
        });
        self.pending = Some(Pending { action, result });
    }

    fn check_reply(&mut self, now: f64) -> Option<SessionGrant> {
        let pending = self.pending.as_ref()?;
        let reply = match pending.result.try_recv() {
            Ok(reply) => reply,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        let Pending { action, .. } = self.pending.take()?;
        self.refresh_at = now + REFRESH_MS;
        self.failed = reply.as_ref().is_none_or(|reply| !reply.ok);
        if self.bridge.is_some() && self.failed && matches!(action, Action::List) {
            // A failed read-only probe can switch stores without replaying a mutation.
            self.bridge = None;
            self.loaded = false;
            self.accounts.clear();
            self.send(Action::List);
            return None;
        }
        match reply {
            Some(reply) if reply.ok => {
                let saved = reply.accounts.is_some();
                if let Some(mut accounts) = reply.accounts {
                    accounts.sort_by_key(|row| !row.current);
                    self.accounts = accounts;
                    self.loaded = true;
                }
                match action {
                    Action::Switch(_) => {
                        if let Some(grant) = reply.session {
                            return Some(grant);
                        }
                        self.apply_failed();
                    }
                    Action::Applied(_) => {
                        self.selected = None;
                        self.status = tr("switched");
                        if self.bridge.is_some() {
                            super::net::reload_handoff();
                        }
                        super::wardrobe::reset();
                        super::relay::reset_for_account_switch();
                    }
                    Action::MicrosoftStart if self.microsoft_form => {
                        self.microsoft_code = reply.code.unwrap_or_default();
                        self.microsoft_url = reply.url.unwrap_or_default();
                        self.microsoft_poll = now + REFRESH_MS;
                        self.status.clear();
                    }
                    Action::AddOffline(_) | Action::NativeLogin(_, _) | Action::MicrosoftStatus
                        if saved =>
                    {
                        let confirm = self.panel == Panel::Add
                            && match action {
                                Action::AddOffline(_) => self.offline,
                                Action::NativeLogin(_, _) => self.native_form,
                                Action::MicrosoftStatus => self.microsoft_form,
                                _ => false,
                            };
                        if confirm {
                            self.panel = Panel::Dropdown;
                            self.clear_form();
                            self.username.clear();
                            self.selected = None;
                        }
                        self.status = tr(if self.bridge.is_some() {
                            "saved"
                        } else {
                            "saved_local"
                        });
                        if confirm
                            && self.bridge.is_none()
                            && let Some(id) = reply.added_account_id
                        {
                            self.send(Action::Switch(id));
                            return None;
                        }
                    }
                    _ => {}
                }
            }
            error if !matches!(action, Action::List | Action::MicrosoftCancel) => {
                self.status = error.map_or_else(|| tr("connection_lost"), |reply| reply.error);
                if matches!(action, Action::MicrosoftStatus) {
                    self.microsoft_code.clear();
                }
            }
            _ => {}
        }
        if let Some(action) = self.queued.take() {
            self.send(action);
        }
        None
    }
}

fn draw_status(c: &mut Canvas, message: &str, x: f32, y: f32, w: f32) {
    if !message.is_empty() {
        let message = c.ellipsize(font::REGULAR, 11.5, message, w);
        c.text(font::REGULAR, 11.5, &message, x, y, theme::TEXT_SECONDARY);
    }
}

fn add_button(ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) -> bool {
    let id = "title:accounts:add";
    let h = 34.0;
    let hover = ui.anim_on("title:accounts:add#h", ui.hover(c, x, y, w, h), 18.0);
    c.round(x, y, w, h, 8.0, theme::alpha(theme::SUBTLE_HOVER, hover));
    c.icon(
        theme::I_PLUS,
        15.0,
        x + 17.0,
        y + h / 2.0,
        theme::TEXT_STRONG,
    );
    let text_y = y + (h - c.line_height(font::SEMIBOLD, 12.0)) / 2.0;
    c.text(
        font::SEMIBOLD,
        12.0,
        &tr("add_account"),
        x + 34.0,
        text_y,
        theme::TEXT_STRONG,
    );
    c.icon(
        theme::I_CHEVRON_RIGHT,
        12.0,
        x + w - 14.0 + 2.0 * hover,
        y + h / 2.0,
        theme::TEXT_SECONDARY,
    );
    ui.clicked(c, id, x, y, w, h)
}

#[allow(clippy::too_many_arguments)]
fn provider_button(
    ui: &mut Ui,
    c: &mut Canvas,
    provider: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    name: &str,
    description: &str,
) -> bool {
    let id = format!("title:accounts:add:{provider}");
    let hover = ui.anim_on(&format!("{id}#h"), ui.hover(c, x, y, w, h), 18.0);
    c.round(x, y, w, h, 11.0, theme::mix(0xC80B0C10, 0xFA16181D, hover));
    c.outline(
        x,
        y,
        w,
        h,
        11.0,
        1.0,
        theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hover),
    );
    let icon = 34.0f32.min(h - 8.0);
    c.round(x + 9.0, y + (h - icon) / 2.0, icon, icon, 8.0, 0xFF101115);
    provider_mark(c, provider, x + 9.0 + icon / 2.0, y + h / 2.0, 17.0);
    let (text_x, text_y) = (x + 21.0 + icon, y + (h - 32.0) / 2.0);
    c.text(
        font::SEMIBOLD,
        13.0,
        name,
        text_x,
        text_y,
        theme::TEXT_STRONG,
    );
    let description = c.ellipsize(font::REGULAR, 11.0, description, w - icon - 52.0);
    c.text(
        font::REGULAR,
        11.0,
        &description,
        text_x,
        text_y + 17.0,
        theme::TEXT_SECONDARY,
    );
    c.icon(
        theme::I_CHEVRON_RIGHT,
        13.0,
        x + w - 17.0 + 2.0 * hover,
        y + h / 2.0,
        theme::TEXT_SECONDARY,
    );
    ui.clicked(c, &id, x, y, w, h)
}

fn provider_mark(c: &mut Canvas, provider: &str, cx: f32, cy: f32, size: f32) {
    match provider {
        "microsoft" => {
            let (x, y) = (cx - size / 2.0, cy - size / 2.0);
            let cell = (size - size / 9.0) / 2.0;
            let next = size - cell;
            c.fill(x, y, cell, cell, 0xFFF25022);
            c.fill(x + next, y, cell, cell, 0xFF7FBA00);
            c.fill(x, y + next, cell, cell, 0xFF00A4EF);
            c.fill(x + next, y + next, cell, cell, 0xFFFFB900);
        }
        "native" => {
            let logo = icons::for_module("watermark", u32::MAX);
            c.icon(logo, size, cx, cy, theme::WHITE);
        }
        "offline" => c.icon(theme::I_WIFI_OFF, size, cx, cy, theme::TEXT_STRONG),
        _ => c.icon(theme::I_USER, size, cx, cy, theme::TEXT_STRONG),
    }
}

fn exchange(address: SocketAddr, request: Value) -> Option<Reply> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(2500)).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(31)))
        .ok()?;
    let mut bytes = serde_json::to_vec(&request).ok()?;
    bytes.push(b'\n');
    stream.write_all(&bytes).ok()?;
    let mut reply = Vec::new();
    BufReader::new(stream.take(262144))
        .read_until(b'\n', &mut reply)
        .ok()?;
    serde_json::from_slice(&reply).ok()
}
