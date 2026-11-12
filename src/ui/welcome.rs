// 何も開いていないときに出す説明（Markdown の Preview で見せる）。

/// 説明のタブの名前（保存先の無いタブの title）。
pub const TITLE: &str = "NeoNote";

/// 説明の本文（今の言語で）。
pub fn text() -> String {
    let en = crate::lang::is_en();
    let (head, desc, ops, rows): (&str, &str, &str, &[(&str, &str, &str)]) = (
        "NeoNote - FileEditor",
        if en {
            "A general-purpose file editor for editing any kind of data freely.  \nBuilt by the captain for convenience. Note of Creation"
        } else {
            "あらゆるデータを自在に編集するための超汎用ファイルエディタ。  \n艦長が利便性を追求した超汎用ファイルエディタ。Note of Creation"
        },
        if en { "Controls" } else { "操作" },
        &[
            ("Ctrl+O", "開く", "Open"),
            ("Ctrl+S", "保存", "Save"),
            ("Ctrl+T", "新しいタブ", "New tab"),
            ("Ctrl+W", "タブを閉じる", "Close the tab"),
            ("Ctrl+Shift+T", "閉じたタブを開き直す", "Reopen a closed tab"),
            ("Ctrl+Tab", "タブの切り替え", "Switch tabs"),
            ("Ctrl+Shift+N", "新しいウィンドウ", "New window"),
            ("Ctrl+F", "検索", "Find"),
            ("F3", "次の一致", "Next match"),
            ("Ctrl+H", "置換", "Replace"),
            ("Ctrl+G", "指定した行へ移動", "Go to line"),
            ("Ctrl+E", "文字コード", "Encoding"),
            ("Ctrl+ホイール", "文字の大きさ", "Ctrl+Wheel: text size"),
            ("F11", "全画面", "Full screen"),
            ("ツールバー", "押すと主な操作、マウスを乗せると関連する操作の一覧", "Toolbar: click for the main action, hover for related actions"),
            ("タブ行の右端", "表示の形式（Format）と文字コード（Encode）の切り替え", "Right end of the tab row: view format and encoding"),
            ("旗・月のボタン", "言語と配色の切り替え", "Flag and moon buttons: language and color theme"),
            ("押しピン・#", "最前面に固定・行番号を常に表示", "Pin and #: keep on top, always show line numbers"),
        ],
    );
    let label = |k: &str| if k.is_ascii() { format!("`{k}`") } else { k.to_string() };
    let mut s = if en {
        format!("# Name: {head}\n\n**About:** {desc}\n\n## ❱ {ops}\n\n")
    } else {
        format!("# 命名：{head}\n\n**説明：** {desc}\n\n## ❱ {ops}\n\n")
    };
    // 特に大事な操作（画面に出ていないので気づきにくい）は、先頭に太字で出す
    let key_ops: &[(&str, &str)] = if en {
        &[
            ("Drag & drop", "Drop files or folders on the window to open them"),
            ("`Alt`", "Show or hide the line numbers and the right panel (they also appear when the mouse reaches the edge)"),
            ("Drag a tab", "Reorder tabs; drop outside the window to move the tab to another window or a new one"),
        ]
    } else {
        &[
            ("ドラッグ＆ドロップ", "ファイルやフォルダをウィンドウへ落とすと開く"),
            ("`Alt`", "行番号と右パネルを開け閉め（マウスを画面の端へ寄せても出る）"),
            ("タブのドラッグ", "並べ替え。ウィンドウの外へ落とすと、別のウィンドウや新しいウィンドウへ移る"),
        ]
    };
    for (k, v) in key_ops {
        s.push_str(&if en { format!("- **{k}** — **{v}**\n") } else { format!("- **{k}：{v}**\n") });
    }
    s.push_str("\n---\n\n");
    for (k, ja, enx) in rows {
        if en {
            // 英語の説明の中に、キーの名前を含めたものがある（ボタンなど）
            if enx.contains(':') && !k.is_ascii() {
                s.push_str(&format!("- {enx}\n"));
            } else {
                s.push_str(&format!("- {} {enx}\n", label(k)));
            }
        } else {
            s.push_str(&format!("- {}：{ja}\n", label(k)));
        }
    }
    s
}
