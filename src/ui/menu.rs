// メニューの表（MenuEntry）。新機能は表に 1 行足すだけで、ボタンと右クリックの両方に出せる。
// `action: None` の項目は「見た目だけ」で、グレーアウトして押せない。
// ラベルに `/` と `&` は使わない（FLTK のメニューが階層・ショートカットとして解釈する）。
use crate::ui::theme::{color_main, color_text, mix};
use fltk::{
    app,
    enums::{Color, Font, FrameType, Shortcut},
    group::Group,
    menu::{MenuButton, MenuFlag},
    prelude::*,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// Enable Auto Save の状態（メニューのチェック表示用）。App が切り替えるたびに更新する。
static AUTOSAVE_ON: AtomicBool = AtomicBool::new(false);
/// Word Wrap の状態（同上）。
static WRAP_ON: AtomicBool = AtomicBool::new(false);

pub fn set_wrap_checked(on: bool) {
    WRAP_ON.store(on, Ordering::Relaxed);
}

pub fn set_autosave_checked(on: bool) {
    AUTOSAVE_ON.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Open,
    OpenFolder,
    Save,
    SaveAs,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Replace,
    Search,
    SearchWeb,
    OpenImageUrl,
    RunCmd,
    ChangeEncoding,
    CopyPath,
    ChooseFont,
    ResetFont,
    OpenReadme,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    OpenHomePage,
    CheckUpdate,
    MakeShortcut,
    OpenRecent,
    OpenConfig,
    ViewHistory,
    RestoreHistory,
    CopyToHistory,
    PasteFromHistory,
    FormatJson,
    ToggleAutoSave,
    ToggleWrap,
    GotoLine,
    ConvertEol,
    ToggleTheme,
    UndoAll,
    RedoAll,
    SelectWord,
    MagicSelect,
    Deselect,
    CopySelection,
    CopyLine,
    PasteBefore,
    PasteAfter,
}

pub struct MenuEntry {
    pub label: &'static str,
    pub action: Option<Action>,
    /// この項目の下に区切り線を引く。
    pub divider: bool,
}

const fn e(label: &'static str, action: Action) -> MenuEntry {
    MenuEntry { label, action: Some(action), divider: false }
}
const fn ed(label: &'static str, action: Action) -> MenuEntry {
    MenuEntry { label, action: Some(action), divider: true }
}

/// 右クリックメニュー（編集領域）
pub const CONTEXT: &[MenuEntry] = &[
    e("Cut", Action::Cut),
    e("Copy", Action::Copy),
    e("Paste", Action::Paste),
    ed("Replace", Action::Replace),
    e("Search", Action::Search),
    e("Search In web", Action::SearchWeb),
    ed("Open Image URL", Action::OpenImageUrl),
    ed("Select All", Action::SelectAll),
    e("Change Encoding", Action::ChangeEncoding),
    ed("Format JSON", Action::FormatJson),
    e("Run In CMD", Action::RunCmd),
    ed("Word Wrap", Action::ToggleWrap),
    e("Go To Line", Action::GotoLine),
    e("Convert Line Ending", Action::ConvertEol),
];

pub const LOAD: &[MenuEntry] = &[
    e("New Tab", Action::NewTab),
    e("Open...", Action::Open),
    e("Open Folder...", Action::OpenFolder),
    ed("Close Tab", Action::CloseTab),
    e("Open Recent", Action::OpenRecent),
    e("Open README", Action::OpenReadme), // 中身は assets/README.txt（今は仮）
    e("Open HomePage", Action::OpenHomePage),
    e("Open Settings", Action::ChooseFont), // フォント選択（Java 版と同じ）
    e("Open Config", Action::OpenConfig),   // neodir/config.txt をタブで開く
    e("Make Shortcut", Action::MakeShortcut),
    ed("Check Update", Action::CheckUpdate),
    e("Exit", Action::Exit),
];
pub const SAVE: &[MenuEntry] = &[
    ed("Save", Action::Save),
    e("Save As...", Action::SaveAs),
    e("Enable Auto Save", Action::ToggleAutoSave),
];
pub const UNDO: &[MenuEntry] = &[
    ed("Undo", Action::Undo),
    e("Undo Last", Action::Undo), // Undo と同じ 1 回分（Java 版にある名前なので残している）
    e("Undo All", Action::UndoAll),
    e("View History", Action::ViewHistory),
];
pub const REDO: &[MenuEntry] = &[
    ed("Redo", Action::Redo),
    e("Redo Last", Action::Redo), // Redo と同じ 1 回分
    e("Redo All", Action::RedoAll),
    e("Restore From History", Action::RestoreHistory),
];
pub const EDIT: &[MenuEntry] = &[
    ed("Select", Action::SelectWord),
    e("Select All", Action::SelectAll),
    e("Magic Select", Action::MagicSelect),
    e("Deselect", Action::Deselect),
    e("Copy Selection", Action::CopySelection),
];
pub const COPY: &[MenuEntry] = &[
    ed("Copy", Action::Copy),
    e("Copy Line", Action::CopyLine),
    e("Copy to Clipboard History", Action::CopyToHistory),
];
pub const PASTE: &[MenuEntry] = &[
    ed("Paste", Action::Paste),
    e("Paste Before", Action::PasteBefore),
    e("Paste After", Action::PasteAfter),
    e("Paste From History", Action::PasteFromHistory),
];
pub const RUN: &[MenuEntry] = &[
    ed("Run In CMD", Action::RunCmd),
    e("Format JSON", Action::FormatJson),
    e("Change Encoding", Action::ChangeEncoding),
    e("Choose Font", Action::ChooseFont),
    e("Default Font", Action::ResetFont),
    e("Toggle Theme", Action::ToggleTheme),
];

/// ポップアップの 1 行分。ラベルはそのまま表示される（`/` `&` などは中で退避するので、パスを含んでいてもよい）。
pub struct Row<'a> {
    pub label: &'a str,
    /// false ならグレーアウトして選べない。
    pub active: bool,
    /// この行の下に区切り線を引く。
    pub divider: bool,
    /// Some なら、チェック付きの行（中身は今の状態）。
    pub check: Option<bool>,
}

impl<'a> Row<'a> {
    pub fn plain(label: &'a str) -> Row<'a> {
        Row { label, active: true, divider: false, check: None }
    }
}

/// FLTK のメニュー記法（`& / \\ _`）を、文字としてそのまま出すために退避する。
fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '&' | '/' | '\\' | '_') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// アプリの配色（main / text / accent）をまとめたメニューを `(x, y)`（ウィンドウ内の座標）の真下に出し、
/// 選ばれた行の番号を返す（選ばずに閉じたら None）。
/// `MenuItem::popup` は部品の色を持てず FLTK 標準の明るい灰色で出るので、色を持てる `MenuButton` を一時的に作って使う。
pub fn pick(rows: &[Row], x: i32, y: i32) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    Group::set_current(None::<&Group>); // どのグループにも入れない（ウィンドウの子にしない）
    let rgb = |(r, g, b): (u8, u8, u8)| Color::from_rgb(r, g, b);
    let mut mb = MenuButton::new(x, y, 1, 1, None);
    mb.set_frame(FrameType::FlatBox); // 枠が無いと、メニューは「ボタンの下」ではなくマウス位置に出てしまう
    mb.set_down_frame(FrameType::FlatBox); // メニュー本体と選択行の枠（BorderBox だと選択行に黒い枠が付く）
    // 編集領域と同じ色だと境目が分からないので、背景を text 側へ少し寄せる
    let (main, text) = (color_main(), color_text());
    let bg = mix(main, text, 8);
    mb.set_color(rgb(bg));
    mb.set_text_color(rgb(text));
    // 選択行は、アクセント色だと FLTK が文字を黒く選んで読みにくいので、History と同じ控えめなグレー系にする
    mb.set_selection_color(rgb(mix(bg, text, 22)));
    for r in rows {
        let mut flags = MenuFlag::Normal;
        if !r.active {
            flags |= MenuFlag::Inactive;
        }
        if r.divider && std::env::var_os("NN_NODIV").is_none() {
            flags |= MenuFlag::MenuDivider;
        }
        if let Some(on) = r.check {
            flags |= MenuFlag::Toggle;
            if on {
                flags |= MenuFlag::Value;
            }
        }
        mb.add(&escape(r.label), Shortcut::None, flags, |_| {});
    }
    // 区切り線は Dark3 + Light3、チェックの四角は Background2 で描かれ、標準では白くて目立つ。出している間だけ、パレットに合う色にする
    let line = mix(bg, text, 30);
    let boxbg = mix(bg, text, 15);
    let swap = [(Color::Dark3, line), (Color::Light3, bg), (Color::Background2, boxbg)];
    let saved = swap.map(|(c, _)| c.to_rgb());
    for (c, (r, g, b)) in swap {
        app::set_color(c, r, g, b);
    }
    let chosen = mb.popup().is_some();
    for ((c, _), (r, g, b)) in swap.into_iter().zip(saved) {
        app::set_color(c, r, g, b);
    }
    let idx = mb.value();
    MenuButton::delete(mb);
    if chosen && idx >= 0 && (idx as usize) < rows.len() { Some(idx as usize) } else { None }
}

/// `pick` で開くメニューのおおよその幅（ピクセル）。ウィンドウの端からはみ出さない位置を決めるのに使う。
pub fn pick_width(rows: &[Row]) -> i32 {
    fltk::draw::set_font(Font::Helvetica, 14); // MenuButton の既定のフォントと大きさ
    let w = rows.iter().map(|r| fltk::draw::width(r.label)).fold(0.0f64, f64::max);
    w.ceil() as i32 + 30 // 左右の余白（チェック欄を含む）
}

/// ポップアップを出し、選ばれた Action を返す。グレーの項目は選べない。
pub fn popup(entries: &[MenuEntry], x: i32, y: i32) -> Option<Action> {
    let rows: Vec<Row> = entries
        .iter()
        .map(|m| Row {
            label: m.label,
            active: m.action.is_some(),
            divider: m.divider,
            check: match m.action {
                Some(Action::ToggleAutoSave) => Some(AUTOSAVE_ON.load(Ordering::Relaxed)),
                Some(Action::ToggleWrap) => Some(WRAP_ON.load(Ordering::Relaxed)),
                _ => None,
            },
        })
        .collect();
    entries.get(pick(&rows, x, y)?).and_then(|m| m.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_protects_fltk_menu_syntax() {
        assert_eq!(escape("a/b&c_d\\e"), "a\\/b\\&c\\_d\\\\e");
        assert_eq!(escape("plain"), "plain");
    }

    #[test]
    fn labels_are_unique_per_menu_and_safe_for_fltk() {
        for t in [CONTEXT, LOAD, SAVE, UNDO, REDO, EDIT, COPY, PASTE, RUN] {
            let mut seen = std::collections::HashSet::new();
            for m in t {
                assert!(seen.insert(m.label), "duplicate label {}", m.label);
                assert!(!m.label.contains(['/', '&']), "bad char in {}", m.label);
            }
        }
    }
}
