// 面積図の四角の中に出す、中身の簡単なプレビュー（GUI なし）。
//
// 拡大して四角が大きくなったときだけ、そのファイルの先頭を読む。文字のファイルなら、行ごとの単語の並び（質感。
// 本文の縮小表示と同じ `runs_of`）と、実際の文字の行を持つ。バイナリは持たず、四角は色だけにする。
// 結果はメモリだけに置き（ディスクには書かない）、上限を超えたら使われていない物から捨てる。
// 読み込みの手順（どの四角を、いつ、どれだけ読むか）は画面側（sidepanel）が決める。
// 画面への接続（sidepanel）で使う。
#![allow(dead_code)]
use super::sidepanel::{runs_of, Runs};
use crate::neo::entrypeek;
use std::collections::HashMap;

/// 1 ファイルで先頭を読む量（バイト）、持つ行数、1 行の桁数の上限。
pub const HEAD_BYTES: usize = 16 * 1024;
pub const MAX_LINES: usize = 160;
pub const MAX_COLS: usize = 120;
/// 置いておく量の上限（バイト）。
pub const CACHE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub enum Preview {
    /// 文字のファイル。`lines` は実際の文字（先頭から）、`runs` は行ごとの単語の並び（開始桁, 長さ）。
    Text { lines: Vec<String>, runs: Vec<Runs> },
    /// バイナリ（NUL を含む・文字として崩れが多い）。
    Binary,
    /// 空のファイル。
    Empty,
    /// 読めなかった（zip の中・権限・消えた、など）。何度も試さないために覚えておく。
    Unavailable,
}

impl Preview {
    /// 読んだ先頭のバイトから作る。
    pub fn from_bytes(bytes: &[u8]) -> Preview {
        match entrypeek::text_lines(bytes, HEAD_BYTES, MAX_LINES, MAX_COLS) {
            None => Preview::Binary,
            Some(l) if l.is_empty() => Preview::Empty,
            Some(lines) => {
                let (runs, _) = runs_of(&lines.join("\n"));
                Preview::Text { lines, runs }
            }
        }
    }

    /// 置いておくのに使うおおよその量（バイト）。
    fn cost(&self) -> usize {
        match self {
            Preview::Text { lines, runs } => 96 + lines.iter().map(|l| l.len() + 24).sum::<usize>() + runs.iter().map(|r| r.len() * 4 + 24).sum::<usize>(),
            _ => 96,
        }
    }
}

/// 道筋 → プレビュー。使われていない物から捨てる（LRU）。
pub struct Cache {
    map: HashMap<String, (Preview, u64)>,
    tick: u64,
    bytes: usize,
    cap: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(CACHE_BYTES)
    }
}

impl Cache {
    pub fn new(cap: usize) -> Cache {
        Cache { map: HashMap::new(), tick: 0, bytes: 0, cap }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// 取り出して、使われた順を更新する。
    pub fn get(&mut self, key: &str) -> Option<&Preview> {
        let e = self.map.get_mut(key)?;
        self.tick += 1;
        e.1 = self.tick;
        Some(&e.0)
    }

    pub fn put(&mut self, key: String, p: Preview) {
        self.tick += 1;
        self.bytes += p.cost();
        if let Some((old, _)) = self.map.insert(key, (p, self.tick)) {
            self.bytes -= old.cost();
        }
        // 上限を超えたら、いちばん使われていない物から捨てる（いま入れた物は残す）
        while self.bytes > self.cap && self.map.len() > 1 {
            let Some(oldest) = self.map.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone()) else { break };
            if let Some((p, _)) = self.map.remove(&oldest) {
                self.bytes -= p.cost();
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_gives_lines_and_runs_of_the_same_length() {
        let p = Preview::from_bytes(b"fn main() {\n    println!(\"hi\");\n}\n");
        let Preview::Text { lines, runs } = p else { panic!("文字のはず") };
        assert_eq!(lines.len(), 3);
        assert_eq!(runs.len(), lines.len(), "行ごとに単語の並びがある");
        assert!(runs[1][0].0 >= 4, "字下げが桁として残る: {:?}", runs[1]);
    }

    #[test]
    fn binary_empty_and_limits() {
        assert_eq!(Preview::from_bytes(b"PK\x03\x04\x00\x00"), Preview::Binary);
        assert_eq!(Preview::from_bytes(&[0xFF; 400]), Preview::Binary);
        assert_eq!(Preview::from_bytes(b""), Preview::Empty);
        let long: String = (0..500).map(|i| format!("line {i} {}\n", "x".repeat(300))).collect();
        let Preview::Text { lines, .. } = Preview::from_bytes(long.as_bytes()) else { panic!() };
        assert!(lines.len() <= MAX_LINES);
        assert!(lines.iter().all(|l| l.chars().count() <= MAX_COLS + 1));
    }

    #[test]
    fn cache_evicts_the_least_recently_used_and_respects_the_cap() {
        let text = |n: usize| Preview::from_bytes("abc def\n".repeat(n).as_bytes());
        let one = text(40).cost();
        let mut c = Cache::new(one * 3 + 10);
        for i in 0..3 {
            c.put(format!("f{i}"), text(40));
        }
        assert_eq!(c.len(), 3);
        c.get("f0"); // f0 を使った → f1 がいちばん古い
        c.put("f3".into(), text(40));
        assert!(c.contains("f0") && c.contains("f2") && c.contains("f3"));
        assert!(!c.contains("f1"), "いちばん使われていない物が捨てられる");
        assert!(c.bytes() <= one * 3 + 10);
        // 上限より大きな 1 つでも、いま入れた物は残る
        let mut tiny = Cache::new(10);
        tiny.put("big".into(), text(200));
        assert!(tiny.contains("big") && tiny.len() == 1);
        c.clear();
        assert_eq!((c.len(), c.bytes()), (0, 0));
    }

    #[test]
    fn replacing_a_key_keeps_the_byte_count_right() {
        let mut c = Cache::new(1 << 20);
        c.put("a".into(), Preview::from_bytes("x\n".repeat(100).as_bytes()));
        let before = c.bytes();
        c.put("a".into(), Preview::Unavailable);
        assert_eq!(c.len(), 1);
        assert!(c.bytes() < before);
        assert_eq!(c.bytes(), Preview::Unavailable.cost());
    }

    #[test]
    fn failures_are_remembered_so_they_are_not_retried() {
        let mut c = Cache::default();
        assert!(!c.contains("zip/inner.txt"));
        c.put("zip/inner.txt".into(), Preview::Unavailable);
        assert_eq!(c.get("zip/inner.txt"), Some(&Preview::Unavailable));
    }
}
