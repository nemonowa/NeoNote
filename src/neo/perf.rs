// 計測用（通常の `cargo test` では動かさない）。CI の計測ジョブと、手元で数値を取るときに使う。
//
//   cargo test --release perf_ -- --ignored --nocapture --test-threads=1
//
// 結果は標準出力へ `PERF key=value ...` の 1 行で出す。docs/MEASURE.md に基準値を残す。
use super::replace;
use fltk::text::TextBuffer;
use std::time::Instant;

/// 現在のプロセスのメモリ（Linux のみ。それ以外は 0）。`VmRSS` は今の常駐、`VmHWM` は最大。KiB。
fn mem_kib(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok())))
        .unwrap_or(0)
}

/// 英数字・日本語・改行が混ざった、およそ `mb` MiB の文書。
fn sample(mb: usize) -> String {
    let unit = "The quick Brown fox jumps over the lazy dog. あいうえお Hello World, hello rust.\n";
    unit.repeat(mb * 1024 * 1024 / unit.len())
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
#[ignore]
fn perf_search() {
    for mb in [8, 50] {
        let text = sample(mb);
        for needle in ["hello", "zzzq"] {
            let t = Instant::now();
            let hits = replace::find_all_smart(&text, needle);
            println!("PERF search mb={mb} needle={needle} hits={} ms={:.1}", hits.len(), ms(t));
        }
        let t = Instant::now();
        let out = replace::replace_all(&text, "hello", "HELLO").map(|(s, n)| (s.len(), n));
        println!("PERF replace_all mb={mb} replaced={:?} ms={:.1}", out.map(|o| o.1), ms(t));
    }
}

/// FLTK の TextBuffer に大きな文書を載せたときの時間とメモリ。項目 5（大きなファイルの閲覧モード）が要るかの判断材料。
#[test]
#[ignore]
fn perf_text_buffer() {
    // macOS の FLTK は、メインスレッドの外で使うと止める（テストは別スレッドで動く。CI で SIGABRT になった）
    if cfg!(target_os = "macos") {
        println!("skip perf_text_buffer on macOS (FLTK needs the main thread)");
        return;
    }
    for mb in [8, 50, 200] {
        let text = sample(mb);
        let before = mem_kib("VmRSS:");
        let mut buf = TextBuffer::default();
        let t = Instant::now();
        buf.set_text(&text);
        let set_ms = ms(t);
        let after = mem_kib("VmRSS:");
        let t = Instant::now();
        let back = buf.text();
        let text_ms = ms(t);
        let t = Instant::now();
        let lines = buf.count_lines(0, buf.length());
        let lines_ms = ms(t);
        println!(
            "PERF text_buffer mb={mb} set_text_ms={set_ms:.1} text_ms={text_ms:.1} count_lines_ms={lines_ms:.1} lines={lines} buffer_extra_mib={} peak_mib={}",
            after.saturating_sub(before) / 1024,
            mem_kib("VmHWM:") / 1024
        );
        assert_eq!(back.len(), text.len());
    }
}

/// フォルダの走査（`archive::list_dir`）とツリーマップの木づくり（`treemap::build`）の時間。
/// 一時フォルダに 1000 フォルダ × 40 ファイル（4 万件）を作って測る。
#[test]
#[ignore]
fn perf_scan() {
    use crate::neo::archive::{list_dir, Entry};
    let root = std::env::temp_dir().join(format!("neonote_perf_scan_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in 0..1000 {
        let dir = root.join(format!("d{d:04}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..40 {
            std::fs::write(dir.join(format!("file{f:03}.dat")), [0u8; 10]).unwrap();
        }
    }
    for round in 0..3 {
        let t = Instant::now();
        let ents = list_dir(&root).unwrap();
        let scan_ms = ms(t);
        let t = Instant::now();
        let tree = crate::ui::treemap::build(&ents);
        println!("PERF scan round={round} entries={} list_dir_ms={scan_ms:.1} treemap_build_ms={:.1} size={}", ents.len(), ms(t), tree.size);
    }
    let _ = std::fs::remove_dir_all(&root);
    // 1 つのフォルダに 4 万ファイル（子の探し方が 2 乗になりやすい形）と、深い入れ子
    let mut wide: Vec<Entry> = (0..40_000).map(|i| Entry { path: format!("big/f{i:06}.bin"), is_dir: false, size: 1 + i as u64 % 97, packed: None }).collect();
    wide.extend((0..2_000).map(|i| Entry { path: format!("dirs/sub{i:05}/x.txt"), is_dir: false, size: 5, packed: None }));
    let t = Instant::now();
    let tree = crate::ui::treemap::build(&wide);
    println!("PERF treemap_build_wide entries={} ms={:.1} size={}", wide.len(), ms(t), tree.size);
}
