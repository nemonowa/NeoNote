// .mcfunction の先頭へ「呼び出し元ヘッダー」を作る（GUI なし。依存も足さない）。
//
//   # 命名：tick
//   # 説明：毎 tick の処理
//   # >/function demo:main
//   # >/advancement demo:start
//   # >/tag/function #minecraft:tick
//   # =/function demo:sub/tick
//
// `>` の行が呼び出し元、`=` の行が自分の ID。この 2 種類の行だけを毎回作り直し、命名・説明・
// 独自の項目（`# 実行条件：…` など）は書いたまま残す。
//
// 呼び出し元を探す範囲は「このファイルが入っているデータパック（`data/` の親）」だけ。NeoNote は
// フォルダやプロジェクトを持たないので、ファイルの場所から `data/<名前空間>/function(s)/…` を
// たどって決める。探す物:
//   - 他の .mcfunction の `function ns:id`（コメント行・引用符の中・say などの自由な文は除く）
//   - advancement の `rewards.function`
//   - enchantment の中にある文字列 `ns:id`（`run_function` など。JSON を読んで完全一致で比べる）
//   - function タグの `values`（`#ns:tag` の入れ子も、循環があっても正しくたどる）
// 検索は 1 パスで、見つけたファイルは 1 回しか読まない。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 読むファイル数の上限（想定外の巨大なフォルダで固まらないため）。
const MAX_FILES: usize = 200_000;
/// 1 ファイルの大きさの上限。これを超えるファイルは読まない。
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// JSON の入れ子の上限。
const MAX_DEPTH: usize = 64;

/// 開いているファイルの、データパック内での位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locate {
    /// データパックの `data` フォルダ。
    pub data_dir: PathBuf,
    /// このファイルの ID（`ns:sub/path`）。
    pub id: String,
}

/// 見つかった呼び出し元（それぞれ自然順に並べ済み）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Callers {
    pub functions: Vec<String>,
    pub advancements: Vec<String>,
    pub enchantments: Vec<String>,
    pub tags: Vec<String>,
}

impl Callers {
    pub fn total(&self) -> usize {
        self.functions.len() + self.advancements.len() + self.enchantments.len() + self.tags.len()
    }
}

/// ヘッダーの書き換え 1 回分。`text[start..end]` を `text` に置き換える。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Edit {
    /// 置き換えても本文が変わらないか。
    pub fn is_noop(&self, doc: &str) -> bool {
        doc.get(self.start..self.end) == Some(self.text.as_str())
    }
}

// ---------------------------------------------------------------- ID

/// `ns:path` に直す（名前空間が無ければ Minecraft の既定の `minecraft`）。
fn norm(raw: &str) -> String {
    if raw.contains(':') { raw.to_string() } else { format!("minecraft:{raw}") }
}

fn is_function_dir(s: &str) -> bool {
    s == "function" || s == "functions"
}

/// `.mcfunction` のパスから、データパックの `data` フォルダと自分の ID を求める。
/// 外側の `data` から順に「`data/<名前空間>/function(s)/…`」の形になる所を探すので、
/// `function/data/init.mcfunction` のように関数の下に `data` という名前のフォルダがあっても、
/// 名前空間を取り違えない。
pub fn locate(path: &Path) -> Option<Locate> {
    let comps: Vec<_> = path.components().collect();
    let names: Vec<String> = comps.iter().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    for i in 0..names.len() {
        if names[i] != "data" || i + 4 > names.len() || !is_function_dir(&names[i + 2]) {
            continue;
        }
        let rest = &names[i + 3..];
        let Some(stem) = rest.last().and_then(|f| f.strip_suffix(".mcfunction")) else { continue };
        if stem.is_empty() {
            continue;
        }
        let mut parts: Vec<&str> = rest[..rest.len() - 1].iter().map(String::as_str).collect();
        parts.push(stem);
        let data_dir: PathBuf = comps[..=i].iter().collect();
        return Some(Locate { data_dir, id: format!("{}:{}", names[i + 1], parts.join("/")) });
    }
    None
}

/// `base/<相対パス><ext>` から `ns:相対パス` を作る。
fn resource_id(ns: &str, base: &Path, file: &Path, ext: &str) -> Option<String> {
    let rel = file.strip_prefix(base).ok()?;
    let mut parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let last = parts.pop()?;
    let stem = last.strip_suffix(ext)?;
    if stem.is_empty() {
        return None;
    }
    parts.push(stem.to_string());
    Some(format!("{ns}:{}", parts.join("/")))
}

// ---------------------------------------------------------------- function の呼び出し

fn is_id_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'/' | b':')
}

/// 自由な文を取るコマンド（`say function a:b` は呼び出しではない）。
const FREE_TEXT: &[&str] = &["say", "me", "tell", "msg", "w", "teammsg", "tm", "kick", "ban", "ban-ip"];

/// コメント行を除き、行末の `\` で続く行を 1 つのコマンドにまとめた行の一覧。
fn commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if cur.is_empty() && line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(head) = line.strip_suffix('\\') {
            cur.push_str(head);
            cur.push(' ');
            continue;
        }
        cur.push_str(line);
        out.push(std::mem::take(&mut cur));
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `text` のどこかに `function <target>` があるか。`target` は `ns:id`。
/// ID は `[A-Za-z0-9_.-/:]` の連なりを丸ごと比べるので、`ns:foo` が `ns:foo-bar` や
/// `ns:foo.bar` に当たることは無い。引用符の中と、say などの自由な文は見ない。
pub fn calls_function(text: &str, target: &str) -> bool {
    for line in commands(text) {
        if line_calls(&line, target) {
            return true;
        }
    }
    false
}

fn line_calls(line: &str, target: &str) -> bool {
    let b = line.as_bytes();
    let first = line.trim_start().trim_start_matches('$');
    let head = first.split_whitespace().next().unwrap_or("");
    if FREE_TEXT.contains(&head) {
        return false;
    }
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        // 語の頭の `function`
        let boundary = i == 0 || !(is_id_char(b[i - 1]) || b[i - 1] == b'#' || b[i - 1] == b'@');
        if boundary && b[i..].starts_with(b"function") {
            let mut j = i + 8;
            if j < b.len() && matches!(b[j], b' ' | b'\t') {
                while j < b.len() && matches!(b[j], b' ' | b'\t') {
                    j += 1;
                }
                let s = j;
                while j < b.len() && is_id_char(b[j]) {
                    j += 1;
                }
                if j > s && norm(&line[s..j]) == target {
                    return true;
                }
                i = j.max(i + 1);
                continue;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------- 小さな JSON 読み

enum J {
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
    Other,
}

struct JP<'a> {
    b: &'a [u8],
    i: usize,
}

impl JP<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    fn value(&mut self, depth: usize) -> Option<J> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match *self.b.get(self.i)? {
            b'"' => self.string().map(J::Str),
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if *self.b.get(self.i)? == b']' {
                        self.i += 1;
                        return Some(J::Arr(v));
                    }
                    v.push(self.value(depth + 1)?);
                    self.ws();
                    match *self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {}
                        _ => return None,
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if *self.b.get(self.i)? == b'}' {
                        self.i += 1;
                        return Some(J::Obj(v));
                    }
                    let k = self.string()?;
                    self.ws();
                    if *self.b.get(self.i)? != b':' {
                        return None;
                    }
                    self.i += 1;
                    v.push((k, self.value(depth + 1)?));
                    self.ws();
                    match *self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b'}' => {}
                        _ => return None,
                    }
                }
            }
            _ => {
                // 数・true・false・null: 区切りまで読み飛ばす
                let s = self.i;
                while self.i < self.b.len() && !matches!(self.b[self.i], b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r') {
                    self.i += 1;
                }
                if self.i == s { None } else { Some(J::Other) }
            }
        }
    }
    fn string(&mut self) -> Option<String> {
        if *self.b.get(self.i)? != b'"' {
            return None;
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' | b'f' => {}
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4)?;
                            self.i += 4;
                            let n = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            let ch = char::from_u32(n).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other), // `"` `\` `/`
                    }
                }
                _ => out.push(c),
            }
        }
    }
}

fn parse_json(text: &str) -> Option<J> {
    let mut p = JP { b: text.as_bytes(), i: 0 };
    p.value(0)
}

fn get<'a>(o: &'a J, key: &str) -> Option<&'a J> {
    match o {
        J::Obj(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// JSON のどこかに、`target`（名前空間なしは minecraft 扱い）と等しい文字列があるか。
fn has_string(j: &J, target: &str) -> bool {
    match j {
        J::Str(s) => norm(s) == target,
        J::Arr(v) => v.iter().any(|x| has_string(x, target)),
        J::Obj(v) => v.iter().any(|(_, x)| has_string(x, target)),
        J::Other => false,
    }
}

/// function タグの `values`（文字列か `{id, required}`）を文字列の一覧にする。
fn tag_values(j: &J) -> Option<Vec<String>> {
    let J::Arr(v) = get(j, "values")? else { return None };
    Some(
        v.iter()
            .filter_map(|x| match x {
                J::Str(s) => Some(s.clone()),
                J::Obj(_) => match get(x, "id") {
                    Some(J::Str(s)) => Some(s.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
    )
}

// ---------------------------------------------------------------- 走査

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// `dir` の下の `ext` で終わるファイルを全部集める（シンボリックリンクは辿らない）。
fn collect(dir: &Path, ext: &str, left: &mut usize, cancel: &AtomicBool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for ent in rd.flatten() {
            let Ok(ft) = ent.file_type() else { continue };
            let p = ent.path();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(ext)) {
                if *left == 0 {
                    return Err(format!("too many files (over {MAX_FILES})"));
                }
                *left -= 1;
                out.push(p);
            }
        }
    }
    Ok(())
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// データパック（`loc.data_dir`）の中から、`loc.id` の呼び出し元を探す。
pub fn find_callers(loc: &Locate, cancel: &AtomicBool) -> Result<Callers, String> {
    let mut left = MAX_FILES;
    let mut out = Callers::default();
    let mut tag_values_by_id: HashMap<String, Vec<String>> = HashMap::new();
    for ns_dir in subdirs(&loc.data_dir) {
        let ns = ns_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // function
        for dn in ["function", "functions"] {
            let base = ns_dir.join(dn);
            let mut files = Vec::new();
            collect(&base, ".mcfunction", &mut left, cancel, &mut files)?;
            for f in files {
                let Some(id) = resource_id(&ns, &base, &f, ".mcfunction") else { continue };
                if id == loc.id {
                    continue;
                }
                if read_small(&f).is_some_and(|t| calls_function(&t, &loc.id)) {
                    out.functions.push(id);
                }
            }
        }
        // advancement
        for dn in ["advancement", "advancements"] {
            let base = ns_dir.join(dn);
            let mut files = Vec::new();
            collect(&base, ".json", &mut left, cancel, &mut files)?;
            for f in files {
                let Some(id) = resource_id(&ns, &base, &f, ".json") else { continue };
                let Some(j) = read_small(&f).and_then(|t| parse_json(&t)) else { continue };
                if let Some(J::Str(s)) = get(&j, "rewards").and_then(|r| get(r, "function")) {
                    if norm(s) == loc.id {
                        out.advancements.push(id);
                    }
                }
            }
        }
        // enchantment（1.21 以降のデータ駆動）
        {
            let base = ns_dir.join("enchantment");
            let mut files = Vec::new();
            collect(&base, ".json", &mut left, cancel, &mut files)?;
            for f in files {
                let Some(id) = resource_id(&ns, &base, &f, ".json") else { continue };
                if read_small(&f).and_then(|t| parse_json(&t)).is_some_and(|j| has_string(&j, &loc.id)) {
                    out.enchantments.push(id);
                }
            }
        }
        // function タグ
        for dn in ["function", "functions"] {
            let base = ns_dir.join("tags").join(dn);
            let mut files = Vec::new();
            collect(&base, ".json", &mut left, cancel, &mut files)?;
            for f in files {
                let Some(id) = resource_id(&ns, &base, &f, ".json") else { continue };
                if let Some(vals) = read_small(&f).and_then(|t| parse_json(&t)).and_then(|j| tag_values(&j)) {
                    tag_values_by_id.insert(id, vals);
                }
            }
        }
    }
    out.tags = resolve_tags(&tag_values_by_id, &loc.id);
    for v in [&mut out.functions, &mut out.advancements, &mut out.enchantments, &mut out.tags] {
        v.sort_by(|a, b| natural_cmp(a, b));
        v.dedup();
    }
    Ok(out)
}

/// `target` に（`#` の入れ子をたどって）行き着くタグの一覧。循環があっても、行き着ける物は必ず入る。
fn resolve_tags(tags: &HashMap<String, Vec<String>>, target: &str) -> Vec<String> {
    let mut hit: HashSet<String> = HashSet::new();
    loop {
        let before = hit.len();
        for (id, vals) in tags {
            if hit.contains(id) {
                continue;
            }
            let reaches = vals.iter().any(|v| match v.strip_prefix('#') {
                Some(nested) => hit.contains(&norm(nested)),
                None => norm(v) == target,
            });
            if reaches {
                hit.insert(id.clone());
            }
        }
        if hit.len() == before {
            break;
        }
    }
    hit.into_iter().collect()
}

/// 自然順（数字の連なりは数として、そのほかは大文字小文字を区別せず）。単体でもフォルダ一括でも同じ順にする。
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < x.len() && x[i].is_ascii_digit() {
                i += 1;
            }
            while j < y.len() && y[j].is_ascii_digit() {
                j += 1;
            }
            let (na, nb) = (a[si..i].trim_start_matches('0'), b[sj..j].trim_start_matches('0'));
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        } else {
            let o = x[i].to_ascii_lowercase().cmp(&y[j].to_ascii_lowercase());
            if o != std::cmp::Ordering::Equal {
                return o;
            }
            i += 1;
            j += 1;
        }
    }
    (x.len() - i).cmp(&(y.len() - j)).then_with(|| a.cmp(b))
}

// ---------------------------------------------------------------- ヘッダーの組み立て

/// `# >/…` `# =/…` `# >(…` の行（作り直す行）か。`#> 説明` のような普通のコメントは含まない。
fn is_generated(line: &str) -> bool {
    let t = line.trim_start();
    let Some(t) = t.strip_prefix('#') else { return false };
    let t = t.trim_start();
    matches!(t.strip_prefix('>').or_else(|| t.strip_prefix('=')).and_then(|r| r.chars().next()), Some('/' | '(' | '（'))
}

fn is_naming(line: &str) -> bool {
    line.trim_start().strip_prefix('#').is_some_and(|t| t.trim_start().starts_with("命名"))
}

fn is_description(line: &str) -> bool {
    line.trim_start().strip_prefix('#').is_some_and(|t| t.trim_start().starts_with("説明"))
}

/// 先頭から続く `#` の行の終わり（バイト位置。末尾の改行を含む）。無ければ 0。
fn leading_comment_end(doc: &str) -> usize {
    let mut end = 0;
    for line in doc.split_inclusive('\n') {
        if line.trim_start().starts_with('#') {
            end += line.len();
        } else {
            break;
        }
    }
    end
}

/// 先頭に付けるヘッダーの置き換えを作る。
/// 先頭の `#` の塊に、作り直す行か `命名` `説明` がある時だけ、それを既存のヘッダーとして置き換える
/// （無ければ、コードの説明コメントを巻き込まないよう、その手前に新しく入れて空行で区切る）。
pub fn build(doc: &str, file_stem: &str, id: &str, callers: &Callers) -> Edit {
    let end = leading_comment_end(doc);
    let block = &doc[..end];
    let is_header = block.lines().any(|l| is_generated(l) || is_naming(l) || is_description(l));
    let kept: Vec<&str> = if is_header { block.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !is_generated(l)).collect() } else { Vec::new() };
    let has_naming = kept.iter().any(|l| is_naming(l));
    let has_desc = kept.iter().any(|l| is_description(l));

    let mut lines: Vec<String> = kept.iter().map(|s| s.to_string()).collect();
    if !has_naming {
        // 命名が無い時だけ、命名と説明の既定値を補う（命名がある時は他の項目も触らない）
        if !has_desc {
            lines.insert(0, "# 説明：".to_string());
        }
        lines.insert(0, format!("# 命名：{file_stem}"));
    }
    if callers.total() == 0 {
        lines.push("# >(呼び出し元が見つかりませんでした)".to_string());
    } else {
        lines.extend(callers.functions.iter().map(|c| format!("# >/function {c}")));
        lines.extend(callers.advancements.iter().map(|c| format!("# >/advancement {c}")));
        lines.extend(callers.enchantments.iter().map(|c| format!("# >/enchantment {c}")));
        lines.extend(callers.tags.iter().map(|c| format!("# >/tag/function #{c}")));
    }
    lines.push(format!("# =/function {id}"));

    let mut text = lines.join("\n");
    text.push('\n');
    if is_header {
        Edit { start: 0, end, text }
    } else {
        // 新しく入れる時は、本文との間を空行で区切る（本文が空、または既に空行で始まる時は足さない）
        if !doc.is_empty() && !doc.starts_with('\n') && !doc.starts_with("\r\n") {
            text.push('\n');
        }
        Edit { start: 0, end: 0, text }
    }
}

// ---------------------------------------------------------------- 本文の中の ID から開く

/// 本文に書かれた `ns:path` が指す物の種類（手前の語から決める。分からなければ `Any`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Function,
    FunctionTag,
    Advancement,
    Enchantment,
    Any,
}

/// 本文の中の `ns:path`（`#ns:tag` を含む）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// 行の中のバイト範囲（`#` は含めない）。
    pub range: std::ops::Range<usize>,
    pub id: String,
    pub kind: RefKind,
}

/// `line` の `at`（バイト位置）にかかっている `ns:path` を返す。`:` を含み、名前空間とパスが
/// 空でなく、パスに `.` `..` や空の区切りが無いものだけ。手前が `function` なら関数、`advancement`
/// ならadvancement、`enchantment` ならエンチャント、`#` ならタグ、それ以外は `Any`。
pub fn reference_at(line: &str, at: usize) -> Option<Reference> {
    let b = line.as_bytes();
    if at >= b.len() || !is_id_char(b[at]) {
        return None;
    }
    let (mut s, mut e) = (at, at);
    while s > 0 && is_id_char(b[s - 1]) {
        s -= 1;
    }
    while e < b.len() && is_id_char(b[e]) {
        e += 1;
    }
    // 文の終わりの `.` や `/`、行頭の `/` は ID に含めない
    while e > s && matches!(b[e - 1], b'.' | b'/' | b'-' | b':') {
        e -= 1;
    }
    while s < e && matches!(b[s], b'/' | b'.' | b'-') {
        s += 1;
    }
    if at < s || at >= e {
        return None;
    }
    let id = &line[s..e];
    let (ns, path) = id.split_once(':')?;
    let ok_ns = !ns.is_empty() && ns.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'));
    let ok_path = !path.is_empty() && path.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.contains(':'));
    if !ok_ns || !ok_path || ns == "." || ns == ".." {
        return None;
    }
    let before = line[..s].trim_end_matches([' ', '\t']);
    let word_is = |w: &str| before.strip_suffix(w).is_some_and(|r| r.as_bytes().last().is_none_or(|&c| !is_id_char(c) || c == b'/'));
    let kind = if line[..s].ends_with('#') {
        RefKind::FunctionTag
    } else if word_is("function") {
        RefKind::Function
    } else if word_is("advancement") {
        RefKind::Advancement
    } else if word_is("enchantment") {
        RefKind::Enchantment
    } else {
        RefKind::Any
    };
    Some(Reference { range: s..e, id: id.to_string(), kind })
}

/// 開く元のファイルの近くにある `data` フォルダの一覧（近い順）。ファイルが入っているデータパックの
/// `data`、上の階層の `data`、そしてそのパックの隣のデータパックの `data`。ID の名前空間が別のパックに
/// あっても見つけられるようにするため。深くまでは探さない。
pub fn data_dirs_near(path: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let push = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if p.is_dir() && !out.contains(&p) {
            out.push(p);
        }
    };
    let mut pack_root: Option<PathBuf> = None;
    for a in path.ancestors().skip(1).take(10) {
        if a.file_name().is_some_and(|n| n == "data") && pack_root.is_none() {
            push(a.to_path_buf(), &mut out);
            pack_root = a.parent().map(Path::to_path_buf);
        } else if a.join("data").is_dir() {
            push(a.join("data"), &mut out);
            if pack_root.is_none() {
                pack_root = Some(a.to_path_buf());
            }
        }
    }
    // 隣のデータパック
    if let Some(parent) = pack_root.as_deref().and_then(Path::parent) {
        let mut sibs = subdirs(parent);
        sibs.sort();
        for sib in sibs {
            push(sib.join("data"), &mut out);
        }
    }
    out
}

/// 物の種類ごとの (フォルダ名の候補, 拡張子)。
fn dirs_of(kind: RefKind) -> Vec<(&'static [&'static str], &'static str)> {
    const FUNC: (&[&str], &str) = (&["function", "functions"], ".mcfunction");
    const TAG: (&[&str], &str) = (&["tags/function", "tags/functions"], ".json");
    const ADV: (&[&str], &str) = (&["advancement", "advancements"], ".json");
    const ENCH: (&[&str], &str) = (&["enchantment"], ".json");
    match kind {
        RefKind::Function => vec![FUNC],
        RefKind::FunctionTag => vec![TAG],
        RefKind::Advancement => vec![ADV],
        RefKind::Enchantment => vec![ENCH],
        RefKind::Any => vec![
            FUNC,
            ADV,
            ENCH,
            (&["loot_table", "loot_tables"], ".json"),
            (&["predicate", "predicates"], ".json"),
            (&["item_modifier", "item_modifiers"], ".json"),
            (&["recipe", "recipes"], ".json"),
            TAG,
        ],
    }
}

/// `r` が指すファイルを、`data_dirs`（近い順）から探す。無ければ None。
/// 種類が分からない時は 関数 → advancement → enchantment → その他の JSON → タグ の順に、先に見つかった物を返す。
pub fn resolve(data_dirs: &[PathBuf], r: &Reference) -> Option<PathBuf> {
    let (ns, path) = r.id.split_once(':')?;
    for (dirs, ext) in dirs_of(r.kind) {
        for data in data_dirs {
            for d in dirs {
                let f = data.join(ns).join(d).join(format!("{path}{ext}"));
                if f.is_file() {
                    return Some(f);
                }
            }
        }
    }
    None
}

/// ファイル名（拡張子なし）。
pub fn stem_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn id_from_path() {
        let l = locate(&p("/w/pack/data/demo/function/sub/tick.mcfunction")).unwrap();
        assert_eq!(l.id, "demo:sub/tick");
        assert_eq!(l.data_dir, p("/w/pack/data"));
        assert_eq!(locate(&p("/w/pack/data/demo/functions/a.mcfunction")).unwrap().id, "demo:a");
        assert!(locate(&p("/w/pack/data/demo/predicate/a.mcfunction")).is_none());
        assert!(locate(&p("/w/pack/data/demo/function/a.txt")).is_none());
    }

    #[test]
    fn id_with_data_folder_inside_function() {
        // 関数の下の `data` を名前空間の基準にしない
        let l = locate(&p("/w/pack/data/demo/function/data/init.mcfunction")).unwrap();
        assert_eq!(l.id, "demo:data/init");
        assert_eq!(l.data_dir, p("/w/pack/data"));
        // パックを置いた場所が `data` という名前でも、正しい `data` を選ぶ
        let l = locate(&p("/data/mypack/data/ns/function/a.mcfunction")).unwrap();
        assert_eq!(l.id, "ns:a");
        assert_eq!(l.data_dir, p("/data/mypack/data"));
    }

    #[test]
    fn call_detection() {
        let t = "say hi\nexecute as @a run function demo:foo\n";
        assert!(calls_function(t, "demo:foo"));
        // 前方一致・`-` `.` の続きは別の ID
        assert!(!calls_function("function demo:foo_bar", "demo:foo"));
        assert!(!calls_function("function demo:foo-bar", "demo:foo"));
        assert!(!calls_function("function demo:foo.bar", "demo:foo"));
        assert!(!calls_function("function demo:foo/bar", "demo:foo"));
        assert!(calls_function("function demo:foo with storage x:y a", "demo:foo"));
        assert!(calls_function("schedule function demo:foo 1t", "demo:foo"));
        assert!(calls_function("$function demo:foo", "demo:foo"));
        // 名前空間を省くと minecraft
        assert!(calls_function("function foo", "minecraft:foo"));
        // コメント・say・引用符の中は呼び出しではない
        assert!(!calls_function("# function demo:foo", "demo:foo"));
        assert!(!calls_function("#> function demo:foo\n# >/function demo:foo", "demo:foo"));
        assert!(!calls_function("say function demo:foo", "demo:foo"));
        assert!(!calls_function("tellraw @a \"function demo:foo\"", "demo:foo"));
        // タグの呼び出しは別物
        assert!(!calls_function("function #demo:foo", "demo:foo"));
        // マクロの ID は比べない
        assert!(!calls_function("$function demo:$(x)", "demo:foo"));
        // 行末の \ で続く行
        assert!(calls_function("execute as @a \\\n  run function demo:foo", "demo:foo"));
    }

    #[test]
    fn tag_resolution_handles_cycles() {
        let mut m: HashMap<String, Vec<String>> = HashMap::new();
        m.insert("a:one".into(), vec!["#a:two".into(), "demo:foo".into()]);
        m.insert("a:two".into(), vec!["#a:one".into()]); // two は one 経由で行き着く
        m.insert("a:three".into(), vec!["#a:two".into()]);
        m.insert("a:none".into(), vec!["demo:other".into()]);
        let mut r = resolve_tags(&m, "demo:foo");
        r.sort();
        assert_eq!(r, vec!["a:one", "a:three", "a:two"]);
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["a:f10", "a:f2", "A:f1", "a:f02"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["A:f1", "a:f02", "a:f2", "a:f10"]);
    }

    fn callers() -> Callers {
        Callers { functions: vec!["demo:main".into()], advancements: vec!["demo:start".into()], enchantments: vec![], tags: vec!["minecraft:tick".into()] }
    }

    #[test]
    fn build_new_header_and_idempotent() {
        let doc = "say hi\n";
        let e = build(doc, "tick", "demo:tick", &callers());
        assert_eq!(
            e.text,
            "# 命名：tick\n# 説明：\n# >/function demo:main\n# >/advancement demo:start\n# >/tag/function #minecraft:tick\n# =/function demo:tick\n\n"
        );
        assert_eq!((e.start, e.end), (0, 0));
        let doc2 = format!("{}{}", e.text, doc);
        let e2 = build(&doc2, "tick", "demo:tick", &callers());
        assert!(e2.is_noop(&doc2), "2 回目は変わらない: {e2:?}");
    }

    #[test]
    fn build_keeps_custom_lines_and_user_comments() {
        let doc = "# 命名：tick\n# 説明：毎tick\n# 実行条件：@a\n#> 備考\n# >/function old:gone\n# =/function old:id\nsay hi\n";
        let e = build(doc, "tick", "demo:tick", &Callers::default());
        assert_eq!(e.text, "# 命名：tick\n# 説明：毎tick\n# 実行条件：@a\n#> 備考\n# >(呼び出し元が見つかりませんでした)\n# =/function demo:tick\n");
        assert_eq!(&doc[e.end..], "say hi\n");
    }

    #[test]
    fn plain_leading_comment_is_not_swallowed() {
        let doc = "# ここで初期化する\nsay hi\n";
        let e = build(doc, "init", "demo:init", &Callers::default());
        assert_eq!((e.start, e.end), (0, 0));
        assert!(e.text.ends_with("# =/function demo:init\n\n"));
    }

    #[test]
    fn empty_doc() {
        let e = build("", "a", "demo:a", &Callers::default());
        assert!(e.text.ends_with("# =/function demo:a\n"));
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let f = root.join(rel);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, body).unwrap();
    }


    #[test]
    fn reference_detection() {
        let l = "execute run function neofunction:asset/bossbar/hide with storage a:b";
        let at = l.find("bossbar").unwrap();
        let r = reference_at(l, at).unwrap();
        assert_eq!(r.id, "neofunction:asset/bossbar/hide");
        assert_eq!(r.kind, RefKind::Function);
        assert_eq!(&l[r.range.clone()], "neofunction:asset/bossbar/hide");
        // 書き方だけ（手前の語なし）
        assert_eq!(reference_at("neofunction:asset/bossbar/hide", 3).unwrap().kind, RefKind::Any);
        // ヘッダーの行・タグ
        assert_eq!(reference_at("# >/function demo:main", 15).unwrap().kind, RefKind::Function);
        assert_eq!(reference_at("# >/advancement demo:start", 20).unwrap().kind, RefKind::Advancement);
        let t = "# >/tag/function #minecraft:tick";
        assert_eq!(reference_at(t, 22).unwrap(), Reference { range: 18..32, id: "minecraft:tick".into(), kind: RefKind::FunctionTag });
        // 文の終わりの `.` は含めない
        let r = reference_at("see demo:a/b.", 6).unwrap();
        assert_eq!(r.id, "demo:a/b");
        // ID でないもの
        assert!(reference_at("http://example.com", 2).is_none());
        assert!(reference_at("demo:../../etc/passwd", 8).is_none());
        assert!(reference_at("demo:a//b", 6).is_none());
        assert!(reference_at("hello world", 2).is_none());
        assert!(reference_at("a:b c", 3).is_none()); // 空白の上
    }

    #[test]
    fn resolve_finds_files_near() {
        let root = std::env::temp_dir().join(format!("neonote_mcref_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write(&root, "packA/data/demo/function/main.mcfunction", "function neofunction:asset/bossbar/hide\n");
        write(&root, "packB/data/neofunction/function/asset/bossbar/hide.mcfunction", "say hide\n");
        write(&root, "packB/data/neofunction/advancement/asset/x.json", "{}");
        write(&root, "packB/data/minecraft/tags/function/tick.json", "{}");
        let from = root.join("packA/data/demo/function/main.mcfunction");
        let dirs = data_dirs_near(&from);
        assert_eq!(dirs[0], root.join("packA/data"));
        assert!(dirs.contains(&root.join("packB/data")));
        let l = "function neofunction:asset/bossbar/hide";
        let r = reference_at(l, 12).unwrap();
        // 隣のパックの関数を見つける
        assert_eq!(resolve(&dirs, &r).unwrap(), root.join("packB/data/neofunction/function/asset/bossbar/hide.mcfunction"));
        let a = reference_at("advancement neofunction:asset/x", 20).unwrap();
        assert!(resolve(&dirs, &a).unwrap().ends_with("advancement/asset/x.json"));
        let t = reference_at("#minecraft:tick", 5).unwrap();
        assert!(resolve(&dirs, &t).unwrap().ends_with("tags/function/tick.json"));
        let none = reference_at("function neofunction:nothing", 12).unwrap();
        assert!(resolve(&dirs, &none).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn end_to_end() {
        let root = std::env::temp_dir().join(format!("neonote_mcheader_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write(&root, "data/demo/function/sub/target.mcfunction", "say target\n");
        write(&root, "data/demo/function/main.mcfunction", "function demo:sub/target\n");
        write(&root, "data/demo/function/data/other.mcfunction", "execute if entity @a run function demo:sub/target\n");
        write(&root, "data/demo/function/decoy.mcfunction", "function demo:sub/target-x\n# function demo:sub/target\nsay function demo:sub/target\n");
        write(&root, "data/demo/advancement/start.json", r#"{"rewards":{"function":"demo:sub/target"}}"#);
        write(&root, "data/demo/advancement/other.json", r#"{"rewards":{"function":"demo:sub/target2"}}"#);
        write(&root, "data/demo/enchantment/ench.json", r#"{"effects":{"minecraft:post_attack":[{"effect":{"type":"minecraft:run_function","function":"demo:sub/target"}}]}}"#);
        write(&root, "data/demo/tags/function/hooks.json", r##"{"values":["demo:sub/target",{"id":"demo:zzz","required":false}]}"##);
        write(&root, "data/minecraft/tags/function/tick.json", r##"{"values":["#demo:hooks"]}"##);
        // 関係の無い JSON が enchantment 扱いされない
        write(&root, "data/demo/predicate/p.json", r#"{"x":"demo:sub/target"}"#);
        let target = root.join("data/demo/function/sub/target.mcfunction");
        let loc = locate(&target).unwrap();
        let c = find_callers(&loc, &AtomicBool::new(false)).unwrap();
        assert_eq!(c.functions, vec!["demo:data/other", "demo:main"]);
        assert_eq!(c.advancements, vec!["demo:start"]);
        assert_eq!(c.enchantments, vec!["demo:ench"]);
        assert_eq!(c.tags, vec!["demo:hooks", "minecraft:tick"]);
        let cancel = AtomicBool::new(true);
        assert!(find_callers(&loc, &cancel).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
