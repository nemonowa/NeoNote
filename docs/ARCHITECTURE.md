# NeoNote の作り

共同開発者向けの概要。ビルドと決まりは [contributing.md](../contributing.md)、機能は [FEATURES.md](FEATURES.md)。

## 決まり

- 描画は fltk-rs だけ。Electron・WebView・ブラウザエンジン・GPU の描画層は使わない。プレビューは FLTK の HelpView。
- 単一の実行ファイル。フォント・アイコンは中に埋め込む。新しい依存は、起動時間と実行ファイルの大きさを測ってから足す。圧縮（deflate）・NBT・ISO・SQLite などの形式は、依存を足さずに読む。
- 重い処理（文字コードの判定・HEX・全置換・フォルダの走査）は GUI の外で行う。
- ファイルの中身を読む処理は、上限と循環の検出を持ち、途中で切ったデータでも落ちないことをテストで確かめる（`neo/fuzz.rs` と各形式のテスト）。
- 入れない物: プロジェクト管理・補完・デバッガ・プラグイン、AI 機能、分割表示。

## 層の分け方

| 場所 | 役割 | 決まり |
| --- | --- | --- |
| `src/neo/` | ファイルの形式・文字の処理・計算 | **GUI（fltk）を import しない**。`cargo test` だけで確かめられる |
| `src/ui/` | 画面の部品（ツールバー・タブ・右の欄・面積図・小窓など） | 計算は `neo/` か、GUI なしの小さなモジュール（`mapview`・`map3d`・`cushion` など）へ出す |
| `src/app.rs` | ウィンドウ・イベント・文書の状態。各部品をつなぐ | |
| `src/platform.rs` | OS ごとに違う処理（ブラウザ・フォルダ・時間帯など） | OS の分岐はここに集める |
| `src/lang.rs` | 日本語・英語 | 画面の文字は `lang::tr("日本語", "English")` を通す |

## 主な仕組み

- **表示の切り替え**（`neo/viewmode.rs`）: ファイルの種類ごとに Text・HEX・Preview から選べる。Preview は Markdown・HTML・CSV・表計算・文書・PDF・画像・音声・NBT など。
- **一覧で開く物**（`neo/archive.rs`）: フォルダ・zip・tar を階層の木と面積図で見る。中を読み取り専用で見る形式（ext・ISO・サムネイルのキャッシュ・アイコン・SQLite・実行ファイルの資源・cab）は `ImageKind` に 1 つ足し、「一覧・1 つ読む・取り出す・全部をたどる」の 4 つの関数を書けば、一覧・検索・小窓・面積図のプレビューがそのまま使える。
- **面積図**（`ui/treemap.rs`・`ui/mapview.rs`・`ui/sidepanel.rs`）: 大きさに比例した四角（squarified）。拡大・移動・小さな項目の畳み込み・立体の陰影・3D・拡大した時の中身のプレビュー。
- **乗せて確かめる小窓**（`neo/peek.rs`・`ui/peekwin.rs`）: 色・文字の参照・UNIX 時刻・日時・進数・Minecraft の色コード・画像のパスなど。
- **文字の変換**（`neo/lineops.rs`・`neo/textconv.rs`）: 並べ替え・大文字小文字と、Unicode・URL・Base64・HTML・色の表記・全角半角などの変換。メニューは `ui/menu.rs` の表に 1 行足す。
- **設定**（`neo/config.rs`）: OS の設定の場所の `config.txt`（普通のテキスト）。知らない行・コメントは残して書き戻す。
- **新しい版の確認**（`neo/update.rs`）: GitHub の最新の公開リリースの版と、`dev::VERSION` を比べる。

## テスト

- `cargo test` で、`neo/` の処理と GUI なしの部品を確かめる。実物から作った小さなデータは `src/neo/testdata/` に置く（個人の情報が入る物は置かない）。
- 計測（`perf_` で始まるテスト）は通常は動かさず、CI で 1 回だけ回す（`cargo test --release perf_ -- --ignored`）。
