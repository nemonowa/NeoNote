# SwingNeoNote を Rust へ移すための資料

- `java/SwingNeoNote.java` … 本体（847行、Swing の単体メモアプリ）。移す対象。
- `java/Log.java` … 本体が使う唯一の neo 内の依存（エラーの記録だけ）。
- `java/FileDropHandler.java` … ゲームから呼ぶ側（ファイルを落とした時に開く）。

ゲームから呼ぶ部分は、Rust 版を別プロセスとして起動する形に置き換える想定。
手元の Rust：`rustc --version` / `cargo --version` で確認。
