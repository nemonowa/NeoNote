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
| 共同開発者向けの資料 | `docs/`（作りの概要 `ARCHITECTURE.md`・機能の一覧 `FEATURES.md`・README の画面） | 入れる |
| 製作者用（リリースの手順・移植元・旧版・作業記録・計測・案） | `admin/` | 入れない（非公開） |
| AI エージェントへの指示 | `CLAUDE.md` | 入れない（非公開） |
| ビルドの出力 | `target/` | 入れない |
| 配布物 | `dist/`（実行ファイルを OS ごとにまとめる） | 入れない |
| 実行時の設定・ログ | 実行ファイルの隣の `neodir/` | 入れない |
| 開発者ごとの物 | `CLAUDE.local.md`、エディタの設定 | 入れない |

## 決まり

- `neo/` は GUI を import しない（`cargo test` だけで確かめられるように）。層の分け方は [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。
- ファイルの中身を読む処理には、上限と循環の検出を付け、途中で切ったデータで落ちないことをテストで確かめる。
- 画面の文字は `lang::tr("日本語", "English")` を通す。
- 確かめていないことは、確かめたと書かない（PR の説明にも「未検証」と書く）。
- 改行は LF（`.gitattributes` でそろえる）。

## 更新履歴

ビルドを出すたびに、[changelog.md](changelog.md) の「内容」の先頭へ、そのバージョンの変更を1行程度で書く。
