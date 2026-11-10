//! The dialogs of the Native server list: Add / Edit server, Direct Connection and Delete. They fill in and answer the
//! game's own multiplayer screens (`Screen::Edit`, `Direct`, `ConfirmDelete`), so what they save and where they
//! connect is the game's, but they are drawn like the rest of Native (the Java mod's `ServersView` dialogs).

use super::servers_view::{bottom_button, tr};
use super::textfield::TextField;
use super::title_view::TitleAct;
use super::ui::{self, Ui};
use crate::native_ui::{Canvas, font, theme};
use horizon_client::title::{Mode, Title};

/// What the game's multiplayer screen is asking: the Native dialog fills it in and answers it (`TitleAct::ServerForm`,
/// `ServerDirect`, `ServerDelete`).
#[derive(Clone, Debug, PartialEq)]
pub enum DialogInfo {
    Edit { add: bool, name: String, address: String, packs: Option<bool> },
    Direct { address: String },
    Delete { name: String },
}

pub fn dialog_info(title: &Title) -> Option<DialogInfo> {
    use horizon_client::multiplayer::Screen;
    let Mode::Multiplayer(screen) = &title.mode else { return None };
    match screen {
        Screen::Edit { index, name, address, packs, .. } => {
            Some(DialogInfo::Edit { add: index.is_none(), name: name.clone(), address: address.clone(), packs: *packs })
        }
        Screen::Direct { address } => Some(DialogInfo::Direct { address: address.clone() }),
        Screen::ConfirmDelete { name, .. } => Some(DialogInfo::Delete { name: name.clone() }),
        _ => None,
    }
}

pub struct ServerDialog {
    name: TextField,
    address: TextField,
    packs: Option<bool>,
    /// Which dialog is open (`add`, `edit`, `direct`, `delete`): a change refills the fields.
    open: Option<&'static str>,
}

impl Default for ServerDialog {
    fn default() -> Self {
        let mut name = TextField::new("servers:dlg:name");
        name.max_length = 64;
        let mut address = TextField::new("servers:dlg:address");
        address.max_length = 255;
        ServerDialog { name, address, packs: None, open: None }
    }
}

fn dialog_box(ui: &mut Ui, c: &mut Canvas, f: &mut TextField, x: f32, y: f32, w: f32, placeholder: &str, paste: bool) -> bool {
    let h = 38.0;
    let focused = f.focused(ui);
    let fa = ui.anim_on(&format!("{}#box", f.id), focused, 14.0);
    let over = ui.hover(c, x, y, w, h);
    c.round(x, y, w, h, 8.0, theme::SUBTLE);
    c.outline(x, y, w, h, 8.0, 1.0, theme::mix(if over { theme::HAIRLINE_STRONG } else { theme::HAIRLINE }, theme::BORDER_HOVER, fa));
    let button_width = if paste { 74.0f32.min((w - 40.0).max(0.0)) } else { 0.0 };
    if paste && button_width > 8.0 {
        let label = tr("native.servers.paste", "Paste");
        if ui.button(c, &format!("{}#paste", f.id), x + w - button_width + 3.0, y + 5.0,
                     button_width - 8.0, 28.0, &label, 0, ui::BTN_GLASS) {
            f.paste_replace(&ui.clip_in);
            f.focus(ui);
            ui.focus_claimed = true;
        }
    }
    f.draw(ui, c, x, y, w - button_width, h, placeholder, 13.5)
}

fn packs_label(packs: Option<bool>) -> String {
    match packs {
        Some(true) => tr("addServer.resourcePack.enabled", "Enabled"),
        Some(false) => tr("addServer.resourcePack.disabled", "Disabled"),
        None => tr("addServer.resourcePack.prompt", "Prompt"),
    }
}

impl ServerDialog {
    /// No dialog is up: the next one starts with fresh fields.
    pub fn closed(&mut self) {
        self.open = None;
    }

    /// Draws the dialog over a dim backdrop and answers it: Enter and the main button submit, Esc and Cancel close.
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &DialogInfo) -> Option<TitleAct> {
        let kind = match info {
            DialogInfo::Edit { add: true, .. } => "add",
            DialogInfo::Edit { add: false, .. } => "edit",
            DialogInfo::Direct { .. } => "direct",
            DialogInfo::Delete { .. } => "delete",
        };
        if self.open != Some(kind) {
            self.open = Some(kind);
            ui.set_anim("servers#dlg", 0.0);
            match info {
                DialogInfo::Edit { name, address, packs, .. } => {
                    self.name.set(name);
                    self.address.set(address);
                    self.packs = *packs;
                    self.address.focus(ui);
                    if !address.is_empty() {
                        self.address.select_all();
                    }
                }
                DialogInfo::Direct { address } => {
                    self.address.set(address);
                    self.address.focus(ui);
                    self.address.select_all();
                }
                DialogInfo::Delete { .. } => ui.focus = None,
            }
        }
        let (w_all, h_all) = (c.width(), c.height());
        let a = ui.anim("servers#dlg", 1.0, 16.0);
        c.fill(0.0, 0.0, w_all, h_all, theme::alpha(0xB8000000, a));
        let w = 420.0f32.min(w_all - 40.0);
        let edit = matches!(info, DialogInfo::Edit { .. });
        let (direct, delete) = (matches!(info, DialogInfo::Direct { .. }), matches!(info, DialogInfo::Delete { .. }));
        let h = if delete { 168.0 } else if direct { 196.0 } else { 330.0 };
        let (x, y) = ((w_all - w) / 2.0, (h_all - h) / 2.0);
        c.push_alpha(a);
        c.round(x, y, w, h, 12.0, 0xFF0A0B0E);
        c.outline(x, y, w, h, 12.0, 1.0, theme::HAIRLINE_STRONG);
        let title = match info {
            DialogInfo::Delete { .. } => tr("selectServer.delete", "Delete"),
            DialogInfo::Direct { .. } => tr("selectServer.direct", "Direct Connection"),
            DialogInfo::Edit { add: false, .. } => tr("addServer.title", "Edit Server Info"),
            DialogInfo::Edit { add: true, .. } => tr("selectServer.add", "Add Server"),
        };
        let shown = c.ellipsize(font::BOLD, 16.0, &title, w - 40.0);
        c.text(font::BOLD, 16.0, &shown, x + 20.0, y + 20.0, theme::TEXT_STRONG);

        let escape = ui.keys.iter().any(|(k, _)| *k == ui::KEY_ESCAPE);
        let enter = ui.keys.iter().any(|(k, _)| *k == ui::KEY_ENTER || *k == ui::KEY_KP_ENTER);
        let mut submit = false;
        let mut fy = y + 56.0;
        if let DialogInfo::Delete { name } = info {
            let n = c.ellipsize(font::SEMIBOLD, 13.5, name, w - 40.0);
            c.text(font::SEMIBOLD, 13.5, &n, x + 20.0, fy, theme::TEXT_STRONG);
            let warning = tr("selectServer.deleteWarning", "'%s' will be lost forever! (A long time!)").replace("%1$s", name).replace("%s", name);
            let warn = c.ellipsize(font::MEDIUM, 11.5, &warning, w - 40.0);
            c.text(font::MEDIUM, 11.5, &warn, x + 20.0, fy + 22.0, theme::TEXT_MUTED);
            submit |= enter;
        } else {
            // the dialog always owns the keyboard: a click on its background must not leave typing going nowhere
            if !self.name.focused(ui) && !self.address.focused(ui) {
                self.address.focus(ui);
            }
            if edit {
                if ui.keys.iter().any(|(k, _)| *k == ui::KEY_TAB) {
                    if self.name.focused(ui) {
                        self.address.focus(ui);
                        self.address.select_all();
                    } else {
                        self.name.focus(ui);
                        self.name.select_all();
                    }
                }
                c.text(font::SEMIBOLD, 10.5, &tr("addServer.enterName", "Server Name").to_uppercase(), x + 20.0, fy, theme::TEXT_MUTED);
                let placeholder = tr("selectServer.defaultName", "Minecraft Server");
                if dialog_box(ui, c, &mut self.name, x + 20.0, fy + 16.0, w - 40.0, &placeholder, false) {
                    if self.address.is_empty() {
                        self.address.focus(ui);
                    } else {
                        submit = true;
                    }
                }
                fy += 66.0;
            }
            c.text(font::SEMIBOLD, 10.5, &tr("addServer.enterIp", "Server Address").to_uppercase(), x + 20.0, fy, theme::TEXT_MUTED);
            if dialog_box(ui, c, &mut self.address, x + 20.0, fy + 16.0, w - 40.0, "play.example.net", true) {
                submit = true;
            }
            if edit {
                // Server Resource Packs: Enabled, Disabled, Prompt
                fy += 66.0;
                c.text(font::SEMIBOLD, 10.5, &tr("addServer.resourcePack", "Server Resource Packs").to_uppercase(), x + 20.0, fy, theme::TEXT_MUTED);
                let label = packs_label(self.packs);
                if ui.button(c, "servers:dlg:packs", x + 20.0, fy + 16.0, w - 40.0, 36.0, &label, 0, ui::BTN_GLASS) {
                    self.packs = match self.packs {
                        Some(true) => Some(false),
                        Some(false) => None,
                        None => Some(true),
                    };
                }
            }
        }

        // footer: Cancel and the main action
        let (bw, bh) = (100.0, 36.0);
        let by = y + h - 20.0 - bh;
        let valid = delete || horizon_protocol::servers::parse_address(&self.address.value()).is_some();
        let ok = match info {
            DialogInfo::Delete { .. } => tr("selectServer.deleteButton", "Delete"),
            DialogInfo::Direct { .. } => tr("selectServer.select", "Join Server"),
            DialogInfo::Edit { .. } => tr("gui.done", "Done"),
        };
        let ok = c.ellipsize(font::SEMIBOLD, 12.5, &ok, bw - 20.0);
        let style = if delete { ui::BTN_DANGER } else { ui::BTN_PRIMARY };
        if bottom_button(ui, c, "servers:dlg:ok", x + w - 20.0 - bw, by, bw, &ok, style, valid) {
            submit = true;
        }
        let cancel = ui.button(c, "servers:dlg:cancel", x + w - 20.0 - bw * 2.0 - 8.0, by, bw, bh, &tr("gui.cancel", "Cancel"), 0, ui::BTN_GLASS);
        c.pop_alpha();

        if escape || cancel {
            return Some(match info {
                DialogInfo::Edit { .. } => TitleAct::ServerForm { name: String::new(), address: String::new(), packs: None, done: false },
                DialogInfo::Direct { .. } => TitleAct::ServerDirect { address: String::new(), join: false },
                DialogInfo::Delete { .. } => TitleAct::ServerDelete { yes: false },
            });
        }
        if submit && valid {
            return Some(match info {
                DialogInfo::Edit { .. } => TitleAct::ServerForm { name: self.name.value(), address: self.address.value(), packs: self.packs, done: true },
                DialogInfo::Direct { .. } => TitleAct::ServerDirect { address: self.address.value(), join: true },
                DialogInfo::Delete { .. } => TitleAct::ServerDelete { yes: true },
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ui::Event;

    fn canvas() -> Canvas {
        let mut canvas = horizon_client::native_ui::canvas();
        canvas.begin(800, 600, 1.0);
        canvas
    }

    #[test]
    fn address_paste_button_replaces_for_direct_and_saved_server_forms() {
        for info in [DialogInfo::Direct { address: "old.example".into() },
                     DialogInfo::Edit { add: true, name: "Server".into(), address: "old.example".into(), packs: None }] {
            let mut dialog = ServerDialog::default();
            let mut ui = Ui::default();
            let mut c = canvas();
            // At 800x600 the button is at x=519..585; address y differs by dialog.
            let y = if matches!(info, DialogInfo::Direct { .. }) { 291.0 } else { 290.0 };
            ui.begin(&[], 1.0);
            dialog.draw(&mut ui, &mut c, &info);
            ui.end();
            ui.begin(&[Event::Move { x: 530.0, y }, Event::Button { button: 0, pressed: true }], 1.0);
            dialog.draw(&mut ui, &mut c, &info);
            ui.end();
            ui.clip_in = "  play.example.net:25565\r\n".into();
            ui.begin(&[Event::Button { button: 0, pressed: false }], 1.0);
            dialog.draw(&mut ui, &mut c, &info);
            ui.end();
            assert_eq!(dialog.address.value(), "play.example.net:25565");
            assert!(dialog.address.focused(&ui));
        }
    }

    #[test]
    fn ctrl_v_pastes_into_the_focused_name_or_address() {
        let mut dialog = ServerDialog::default();
        let mut ui = Ui::default();
        let mut c = canvas();
        let info = DialogInfo::Edit { add: true, name: "Old".into(), address: "old.example".into(), packs: None };
        ui.begin(&[], 1.0);
        dialog.draw(&mut ui, &mut c, &info);
        ui.end();
        ui.clip_in = "new.example".into();
        ui.begin(&[Event::Key {code: 86, mods: ui::MOD_CONTROL, pressed: true}], 1.0);
        dialog.draw(&mut ui, &mut c, &info);
        ui.end();
        assert_eq!(dialog.address.value(), "new.example");
        dialog.name.focus(&mut ui);
        dialog.name.select_all();
        ui.clip_in = "My server".into();
        ui.begin(&[Event::Key {code: 86, mods: ui::MOD_CONTROL, pressed: true}], 1.0);
        dialog.draw(&mut ui, &mut c, &info);
        assert_eq!(dialog.name.value(), "My server");
    }
}
