//! キーと修飾の組。照合・取り込み・ファイルの読み書き・画面の文字を、同じ決まりで作る。

use egui::Key;

/// キーと修飾（Shift・Cmd）の組。`command` は macOS では Cmd、ほかでは Ctrl。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub shift: bool,
    pub command: bool,
}

/// 割り当てられない理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// Esc・Enter・Delete・Backspace。
    Fixed,
    /// Tab・Space・矢印。
    Navigation,
    /// Cmd+C・X・V・A。
    TextEditing,
    /// Cmd+Q・W・H・M。
    System,
    /// 使えるキー（英字・数字・F1〜F12・Home など）でない。
    KeyNotAllowed,
    /// Shift と数字の組。
    ShiftDigit,
    /// Alt（Option）、macOS の Control、修飾キーだけ。
    Modifier,
    /// ファイルの書き方が違う。
    Syntax,
}

impl Refusal {
    /// 画面に出す理由。
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Fixed => {
                "Esc・Enter・Delete・Backspace は役目が決まっているため変えられません"
            }
            Refusal::Navigation => "Tab・Space・矢印は画面の部品の移動に使うため使えません",
            Refusal::TextEditing => {
                "入力欄の切り取り・コピー・貼り付け・全選択に使うため使えません"
            }
            Refusal::System => "終了やウィンドウの操作に使うため使えません",
            Refusal::KeyNotAllowed => {
                "このキーは割り当てに使えません（英字・数字・F1〜F12・Home など）"
            }
            Refusal::ShiftDigit => "Shift と数字の組は、配列によって別の記号になるため使えません",
            Refusal::Modifier => "使える修飾は Shift と Cmd（Ctrl）です",
            Refusal::Syntax => "キーの書き方が違うため読めません",
        }
    }
}

/// 画面の文字の書き分け（Cmd か Ctrl か）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Mac,
    Other,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else {
            Platform::Other
        }
    }

    /// 修飾の名前（「Cmd」「Ctrl」）。
    pub fn command_name(self) -> &'static str {
        match self {
            Platform::Mac => "Cmd",
            Platform::Other => "Ctrl",
        }
    }
}

const DIGITS: [Key; 10] = [
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

fn is_letter(key: Key) -> bool {
    let name = key.name();
    name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase()
}

fn is_digit(key: Key) -> bool {
    DIGITS.contains(&key)
}

/// 要件4.1 の使えるキー。
fn is_allowed(key: Key) -> bool {
    is_letter(key)
        || is_digit(key)
        || matches!(
            key,
            Key::F1
                | Key::F2
                | Key::F3
                | Key::F4
                | Key::F5
                | Key::F6
                | Key::F7
                | Key::F8
                | Key::F9
                | Key::F10
                | Key::F11
                | Key::F12
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Insert
        )
}

impl Chord {
    /// 押されたキーから組を作る。取り込みと照合の両方がここを通る（取り込めた組は必ず働く）。
    pub fn from_event(key: Key, modifiers: egui::Modifiers) -> Result<Chord, Refusal> {
        if modifiers.alt || (modifiers.ctrl && (!modifiers.command || modifiers.mac_cmd)) {
            return Err(Refusal::Modifier);
        }
        Chord {
            key,
            shift: modifiers.shift,
            command: modifiers.command,
        }
        .check()
    }

    /// 割り当てられる組か。
    fn check(self) -> Result<Chord, Refusal> {
        match self.key {
            Key::Escape | Key::Enter | Key::Delete | Key::Backspace => return Err(Refusal::Fixed),
            Key::Tab
            | Key::Space
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::ArrowLeft
            | Key::ArrowRight => return Err(Refusal::Navigation),
            _ => {}
        }
        if self.command && !self.shift {
            if matches!(self.key, Key::C | Key::X | Key::V | Key::A) {
                return Err(Refusal::TextEditing);
            }
            if matches!(self.key, Key::Q | Key::W | Key::H | Key::M) {
                return Err(Refusal::System);
            }
        }
        if self.shift && is_digit(self.key) {
            return Err(Refusal::ShiftDigit);
        }
        if !is_allowed(self.key) {
            return Err(Refusal::KeyNotAllowed);
        }
        Ok(self)
    }

    /// ファイルの書き方（「Shift+Command+Z」）を読む。修飾は Shift・Command の順で、キーは
    /// `egui::Key::name()` と同じ文字。読めた組も `from_event` と同じ決まりで確かめる。
    pub fn parse(text: &str) -> Result<Chord, Refusal> {
        let mut parts: Vec<&str> = text.split('+').collect();
        let Some(name) = parts.pop() else {
            return Err(Refusal::Syntax);
        };
        let mut shift = false;
        let mut command = false;
        for part in parts {
            match part {
                "Shift" if !shift && !command => shift = true,
                "Command" if !command => command = true,
                "Alt" | "Option" | "Control" | "Ctrl" => return Err(Refusal::Modifier),
                _ => return Err(Refusal::Syntax),
            }
        }
        match name {
            "Shift" | "Command" | "Alt" | "Option" | "Control" | "Ctrl" => {
                return Err(Refusal::Modifier);
            }
            _ => {}
        }
        let key = Key::from_name(name)
            .filter(|key| key.name() == name)
            .ok_or(Refusal::Syntax)?;
        Chord {
            key,
            shift,
            command,
        }
        .check()
    }

    /// `parse` の逆。
    pub fn to_file(self) -> String {
        let mut out = String::new();
        if self.shift {
            out.push_str("Shift+");
        }
        if self.command {
            out.push_str("Command+");
        }
        out.push_str(self.key.name());
        out
    }

    /// 画面の文字（「Shift+Cmd+Z」「Shift+Ctrl+Z」）。つなぎは半角の「+」。
    pub fn label(self, platform: Platform) -> String {
        let mut out = String::new();
        if self.shift {
            out.push_str("Shift+");
        }
        if self.command {
            out.push_str(platform.command_name());
            out.push('+');
        }
        out.push_str(self.key.name());
        out
    }

    /// 修飾が無いか。
    pub fn is_plain(self) -> bool {
        !self.shift && !self.command
    }
}

#[cfg(test)]
mod tests {
    use egui::{Key, Modifiers};

    use super::{Chord, Platform, Refusal};

    fn chord(key: Key, shift: bool, command: bool) -> Chord {
        Chord {
            key,
            shift,
            command,
        }
    }

    fn mods(shift: bool, command: bool) -> Modifiers {
        Modifiers {
            alt: false,
            ctrl: command,
            shift,
            mac_cmd: false,
            command,
        }
    }

    #[test]
    fn chord_round_trips_through_file_text() {
        let mut count = 0;
        for &key in Key::ALL {
            for (shift, command) in [(false, false), (true, false), (false, true), (true, true)] {
                let Ok(chord) = Chord::from_event(key, mods(shift, command)) else {
                    continue;
                };
                count += 1;
                let text = chord.to_file();
                assert_eq!(Chord::parse(&text), Ok(chord), "{text}");
            }
        }
        // 英字26・数字10・F1〜F12・Home など5つ。数字は Shift を断る。Cmd だけの C・X・V・A と
        // Q・W・H・M の8つも断る。
        assert_eq!(count, (26 + 12 + 5) * 4 + 10 * 2 - 8, "取り込める組の数");
        assert_eq!(chord(Key::Z, true, true).to_file(), "Shift+Command+Z");
        assert_eq!(chord(Key::Num1, false, false).to_file(), "1");
        assert_eq!(chord(Key::PageUp, false, true).to_file(), "Command+PageUp");
        for bad in [
            "",
            "+",
            "Shift+",
            "Command+Shift+Z",
            "Shift+Shift+Z",
            "Command+Command+Z",
            "z",
            "Numpad1",
            "Cmd+Z",
            "Shift Z",
            "Super+Z",
        ] {
            assert_eq!(Chord::parse(bad), Err(Refusal::Syntax), "{bad:?}");
        }
        for modifier in ["Shift", "Command", "Shift+Command", "Alt+Z", "Control+Z"] {
            assert_eq!(Chord::parse(modifier), Err(Refusal::Modifier), "{modifier}");
        }
    }

    #[test]
    fn refusals_cover_the_table() {
        let plain = mods(false, false);
        let command = mods(false, true);
        for key in [Key::Escape, Key::Enter, Key::Delete, Key::Backspace] {
            assert_eq!(Chord::from_event(key, plain), Err(Refusal::Fixed));
            assert_eq!(Chord::from_event(key, command), Err(Refusal::Fixed));
        }
        for key in [
            Key::Tab,
            Key::Space,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
        ] {
            assert_eq!(Chord::from_event(key, plain), Err(Refusal::Navigation));
        }
        for key in [Key::C, Key::X, Key::V, Key::A] {
            assert_eq!(Chord::from_event(key, command), Err(Refusal::TextEditing));
            assert!(Chord::from_event(key, plain).is_ok());
        }
        for key in [Key::Q, Key::W, Key::H, Key::M] {
            assert_eq!(Chord::from_event(key, command), Err(Refusal::System));
        }
        for key in [Key::Num0, Key::Num5, Key::Num9] {
            assert_eq!(
                Chord::from_event(key, mods(true, false)),
                Err(Refusal::ShiftDigit)
            );
        }
        let alt = Modifiers { alt: true, ..plain };
        assert_eq!(Chord::from_event(Key::G, alt), Err(Refusal::Modifier));
        // macOS の Control（command ではない ctrl）。
        let control = Modifiers {
            ctrl: true,
            ..plain
        };
        assert_eq!(Chord::from_event(Key::G, control), Err(Refusal::Modifier));
        for key in [Key::Minus, Key::Period, Key::Comma, Key::F13, Key::Copy] {
            assert_eq!(Chord::from_event(key, plain), Err(Refusal::KeyNotAllowed));
        }
        // ファイルからも同じ理由で断る（OSによらない）。
        assert_eq!(Chord::parse("Command+C"), Err(Refusal::TextEditing));
        assert_eq!(Chord::parse("Command+Q"), Err(Refusal::System));
        assert_eq!(Chord::parse("Shift+3"), Err(Refusal::ShiftDigit));
        assert_eq!(Chord::parse("Escape"), Err(Refusal::Fixed));
        assert_eq!(Chord::parse("Minus"), Err(Refusal::KeyNotAllowed));
        for refusal in [
            Refusal::Fixed,
            Refusal::Navigation,
            Refusal::TextEditing,
            Refusal::System,
            Refusal::KeyNotAllowed,
            Refusal::ShiftDigit,
            Refusal::Modifier,
            Refusal::Syntax,
        ] {
            assert!(!refusal.message().is_empty());
        }
    }

    #[test]
    fn mac_control_and_command_cannot_trigger_undo() {
        let both = Modifiers {
            ctrl: true,
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        assert_eq!(Chord::from_event(Key::Z, both), Err(Refusal::Modifier));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let z = Chord::from_event(Key::Z, mods(false, false)).unwrap();
        let cmd_z = Chord::from_event(Key::Z, mods(false, true)).unwrap();
        let shift_z = Chord::from_event(Key::Z, mods(true, false)).unwrap();
        assert_ne!(z, cmd_z);
        assert_ne!(z, shift_z);
        assert!(z.is_plain() && !cmd_z.is_plain());
        // macOS の Cmd（mac_cmd と command）も command として扱う。
        let mac = Modifiers {
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        assert_eq!(Chord::from_event(Key::Z, mac), Ok(cmd_z));
    }

    #[test]
    fn labels_follow_the_platform() {
        let redo = chord(Key::Z, true, true);
        assert_eq!(redo.label(Platform::Mac), "Shift+Cmd+Z");
        assert_eq!(redo.label(Platform::Other), "Shift+Ctrl+Z");
        assert_eq!(chord(Key::F1, false, false).label(Platform::Mac), "F1");
        assert_eq!(chord(Key::Num1, false, false).label(Platform::Other), "1");
        assert_eq!(
            chord(Key::PageUp, false, false).label(Platform::Mac),
            "PageUp"
        );
    }
}
