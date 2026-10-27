# 開発の手引き

## 必要なもの

- Rust（stable）
- cmake と C++ コンパイラ（fltk-rs のビルドに使う。Windows は Visual Studio Build Tools）
- Linux では X11 / Wayland の開発用パッケージ（fltk-rs の README を参照）

## ビルドとテスト

```text
cargo build --release
cargo test --release
```

実行ファイルは `target/release/neonote`（Windows は `neonote.exe`）。フォント・アイコン・README は中に埋め込まれるので、配布はこの 1 ファイルだけでよい。

## フォルダの分け方

| 分類 | 場所 | git |
| --- | --- | --- |
| ソース | `src/`、`build.rs`、`Cargo.toml`、`Cargo.lock` | 入れる |
| 埋め込む素材 | `assets/`（フォント・アイコン・Open README の文） | 入れる |
| 同梱の C ライブラリ | `vendor/miniaudio/` | 入れる |
| 設計と記録 | `docs/`（`DESIGN.md`、検証のスクリーンショット） | 入れる |
| 移植元とゲーム側の部品 | `reference/java/`、`integration/java/` | 入れる |
| AI エージェントへの共通の指示 | `CLAUDE.md` | 入れる |
| ビルドの出力 | `target/` | 入れない |
| 配布物 | `dist/`（実行ファイルを OS ごとにまとめる） | 入れない |
| 実行時の設定・ログ | 実行ファイルの隣の `neodir/` | 入れない |
| 開発者ごとの物 | `local/`、`CLAUDE.local.md`、エディタの設定 | 入れない |

## 決まり

- 設計と進捗は `docs/DESIGN.md` に書く。確かめていないことは「未検証」と書く。
- 改行は LF（`.gitattributes` でそろえる）。
