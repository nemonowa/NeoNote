// SQLite のデータベース（先頭が `SQLite format 3\0`）を、表ごとの CSV の一覧として読み取り専用で開く（GUI なし。部品を足さない）。
// 一覧: `schema.sql`（CREATE 文すべて）と、表ごとの `<表の名前>.csv`（1 行目は列の名前）。
// 読むのは表の B 木（rowid の表）だけ。WITHOUT ROWID の表（索引の B 木に入る）は中身を出さず、schema.sql にだけ出る。
// 書きかけの変更（-wal・-journal のファイル）は読まない。ページのたどりは、同じページを 2 度通らないようにして、壊れた入力で回り続けない。
use super::archive::Entry;
use std::collections::HashSet;
use std::path::Path;

const MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// 1 つの表から出す行の上限（それより後ろは省く。一覧が固まらないように）。
const MAX_ROWS: usize = 200_000;
/// 読むデータベースの大きさの上限。
const MAX_FILE: u64 = 512 << 20;

pub fn is_sqlite_path(p: &Path) -> bool {
    use std::io::Read;
    if p.is_dir() {
        return false;
    }
    let mut head = [0u8; 16];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == MAGIC
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

struct Db<'a> {
    b: &'a [u8],
    page_size: usize,
    usable: usize,
    utf16: Option<bool>, // None = UTF-8、Some(true) = UTF-16LE、Some(false) = UTF-16BE
}

fn err<T>(ja: &'static str, en: &'static str) -> Result<T, String> {
    Err(crate::lang::tr(ja, en).to_string())
}

fn varint(b: &[u8], p: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let c = *b.get(p + i)?;
        if i == 8 {
            return Some((v << 8 | c as u64, 9));
        }
        v = v << 7 | (c & 0x7F) as u64;
        if c & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

impl<'a> Db<'a> {
    fn open(b: &'a [u8]) -> Result<Db<'a>, String> {
        if b.len() < 100 || &b[..16] != MAGIC {
            return err("SQLite のデータベースではありません", "Not an SQLite database");
        }
        let ps = u16::from_be_bytes([b[16], b[17]]) as usize;
        let page_size = if ps == 1 { 65536 } else { ps };
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return err("ページの大きさが読めません", "Bad page size");
        }
        let usable = page_size - b[20] as usize;
        let utf16 = match u32::from_be_bytes([b[56], b[57], b[58], b[59]]) {
            2 => Some(true),
            3 => Some(false),
            _ => None,
        };
        Ok(Db { b, page_size, usable, utf16 })
    }

    fn page(&self, n: u32) -> Option<&'a [u8]> {
        let n = n as usize;
        if n == 0 {
            return None;
        }
        self.b.get((n - 1) * self.page_size..n * self.page_size)
    }

    /// 表の B 木の、すべての行（rowid と、記録の中身）。
    fn rows(&self, root: u32, limit: usize, out: &mut Vec<(i64, Vec<u8>)>) -> Result<bool, String> {
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        let mut cut = false;
        // 左から順に読むため、子は逆順に積む
        while let Some(pg) = stack.pop() {
            if !seen.insert(pg) {
                return err("ページが循環しています（壊れています）", "Page loop (corrupt)");
            }
            let Some(page) = self.page(pg) else { return err("ページの番号が外です（壊れています）", "Page out of range (corrupt)") };
            let h = if pg == 1 { 100 } else { 0 };
            let kind = *page.get(h).unwrap_or(&0);
            let n = u16::from_be_bytes([*page.get(h + 3).unwrap_or(&0), *page.get(h + 4).unwrap_or(&0)]) as usize;
            let ptrs = |i: usize, hdr: usize| -> Option<usize> { page.get(h + hdr + i * 2..h + hdr + i * 2 + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize) };
            match kind {
                0x05 => {
                    let right = page.get(h + 8..h + 12).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).unwrap_or(0);
                    let mut kids = Vec::with_capacity(n + 1);
                    for i in 0..n {
                        let c = ptrs(i, 12).ok_or("壊れています")?;
                        kids.push(page.get(c..c + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or("壊れています")?);
                    }
                    kids.push(right);
                    stack.extend(kids.into_iter().rev());
                }
                0x0D => {
                    for i in 0..n {
                        if out.len() >= limit {
                            cut = true;
                            return Ok(cut);
                        }
                        let c = ptrs(i, 8).ok_or("壊れています")?;
                        let (plen, a) = varint(page, c).ok_or("壊れています")?;
                        let (rowid, r) = varint(page, c + a).ok_or("壊れています")?;
                        let payload = self.payload(page, c + a + r, plen as usize)?;
                        out.push((rowid as i64, payload));
                    }
                }
                _ => return err("表の形が読めません（WITHOUT ROWID の表など）", "Unsupported table (e.g. WITHOUT ROWID)"),
            }
        }
        Ok(cut)
    }

    /// セルの中身（はみ出した分は、続きのページをたどる）。
    fn payload(&self, page: &[u8], at: usize, len: usize) -> Result<Vec<u8>, String> {
        let u = self.usable;
        let x = u - 35;
        let local = if len <= x {
            len
        } else {
            let m = (u - 12) * 32 / 255 - 23;
            let k = m + (len - m) % (u - 4);
            if k <= x { k } else { m }
        };
        let mut out = page.get(at..at + local).ok_or("壊れています")?.to_vec();
        if local < len {
            let mut next = page.get(at + local..at + local + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or("壊れています")?;
            let mut seen = HashSet::new();
            while out.len() < len {
                if next == 0 || !seen.insert(next) {
                    return err("続きのページが壊れています", "Broken overflow pages");
                }
                let p = self.page(next).ok_or("壊れています")?;
                let take = (len - out.len()).min(u - 4);
                out.extend_from_slice(p.get(4..4 + take).ok_or("壊れています")?);
                next = u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
            }
        }
        Ok(out)
    }

    fn text(&self, b: &[u8]) -> String {
        match self.utf16 {
            None => String::from_utf8_lossy(b).into_owned(),
            Some(le) => {
                let units: Vec<u16> = b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
                String::from_utf16_lossy(&units)
            }
        }
    }

    /// 記録（列の値の並び）を読む。
    fn record(&self, r: &[u8]) -> Result<Vec<Value>, String> {
        let (hlen, a) = varint(r, 0).ok_or("壊れています")?;
        let mut types = Vec::new();
        let mut p = a;
        while p < hlen as usize {
            let (t, n) = varint(r, p).ok_or("壊れています")?;
            types.push(t);
            p += n;
        }
        let mut q = hlen as usize;
        let mut out = Vec::with_capacity(types.len());
        for t in types {
            let int = |n: usize, q: usize| -> Option<i64> {
                let s = r.get(q..q + n)?;
                let mut v: i64 = if s[0] & 0x80 != 0 { -1 } else { 0 };
                for &c in s {
                    v = v << 8 | c as i64;
                }
                Some(v)
            };
            let (v, n) = match t {
                0 => (Value::Null, 0),
                1..=4 => (Value::Int(int(t as usize, q).ok_or("壊れています")?), t as usize),
                5 => (Value::Int(int(6, q).ok_or("壊れています")?), 6),
                6 => (Value::Int(int(8, q).ok_or("壊れています")?), 8),
                7 => (Value::Real(f64::from_bits(int(8, q).ok_or("壊れています")? as u64)), 8),
                8 => (Value::Int(0), 0),
                9 => (Value::Int(1), 0),
                10 | 11 => (Value::Null, 0),
                t if t % 2 == 0 => {
                    let n = ((t - 12) / 2) as usize;
                    (Value::Blob(r.get(q..q + n).ok_or("壊れています")?.to_vec()), n)
                }
                t => {
                    let n = ((t - 13) / 2) as usize;
                    (Value::Text(self.text(r.get(q..q + n).ok_or("壊れています")?)), n)
                }
            };
            out.push(v);
            q += n;
        }
        Ok(out)
    }
}

/// 表の定義（sqlite_master の 1 行）。
struct Table {
    name: String,
    root: u32,
    sql: String,
}

fn schema(db: &Db) -> Result<(Vec<Table>, Vec<String>), String> {
    let mut rows = Vec::new();
    db.rows(1, usize::MAX, &mut rows)?;
    let mut tables = Vec::new();
    let mut sqls = Vec::new();
    for (_, r) in rows {
        let v = db.record(&r)?;
        let s = |i: usize| match v.get(i) {
            Some(Value::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let root = match v.get(3) {
            Some(Value::Int(n)) => *n as u32,
            _ => 0,
        };
        if !s(4).is_empty() {
            sqls.push(format!("{};", s(4)));
        }
        let without_rowid = s(4).to_ascii_lowercase().replace(char::is_whitespace, "").ends_with("withoutrowid");
        if s(0) == "table" && root > 0 && !without_rowid && !s(1).starts_with("sqlite_") {
            tables.push(Table { name: s(1), root, sql: s(4) });
        }
    }
    Ok((tables, sqls))
}

/// CREATE TABLE の文から、列の名前と、rowid の別名（INTEGER PRIMARY KEY）の列の番号。
fn columns(sql: &str) -> (Vec<String>, Option<usize>) {
    let (Some(s), Some(e)) = (sql.find('('), sql.rfind(')')) else { return (Vec::new(), None) };
    if e <= s {
        return (Vec::new(), None);
    }
    let body = &sql[s + 1..e];
    // 括弧と引用符の外の , で分ける
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut cur) = (0i32, None::<char>, String::new());
    for c in body.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, '[') => quote = Some(']'),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    let mut names = Vec::new();
    let mut alias = None;
    for p in parts {
        let t = p.trim();
        let low = t.to_ascii_lowercase();
        if ["primary ", "unique", "check", "foreign ", "constraint "].iter().any(|k| low.starts_with(k)) || t.is_empty() {
            continue;
        }
        let name = match t.chars().next() {
            Some(q @ ('"' | '`' | '[' | '\'')) => {
                let close = if q == '[' { ']' } else { q };
                t[1..].split(close).next().unwrap_or("").to_string()
            }
            _ => t.split_whitespace().next().unwrap_or("").to_string(),
        };
        let compact: String = low.split_whitespace().collect::<Vec<_>>().join(" ");
        if compact.contains(" integer primary key") && !compact.contains(" desc") {
            alias = Some(names.len());
        }
        names.push(name);
    }
    (names, alias)
}

fn csv_cell(v: &Value) -> String {
    let s = match v {
        Value::Null => return String::new(),
        Value::Int(n) => return n.to_string(),
        Value::Real(f) => return f.to_string(),
        Value::Text(t) => t.clone(),
        Value::Blob(b) => format!("x'{}'", b.iter().take(4096).map(|c| format!("{c:02X}")).collect::<String>()),
    };
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

fn table_csv(db: &Db, t: &Table) -> Result<String, String> {
    let (cols, alias) = columns(&t.sql);
    let mut rows = Vec::new();
    let cut = db.rows(t.root, MAX_ROWS, &mut rows)?;
    let mut out = String::new();
    out.push_str(&cols.iter().map(|c| csv_cell(&Value::Text(c.clone()))).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for (rowid, r) in rows {
        let mut v = db.record(&r)?;
        // 列を足した後の古い行は、後ろの列が記録に無い（既定値の代わりに空にする）
        while v.len() < cols.len() {
            v.push(Value::Null);
        }
        if let Some(i) = alias {
            if v.get(i) == Some(&Value::Null) {
                v[i] = Value::Int(rowid);
            }
        }
        out.push_str(&v.iter().map(csv_cell).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    if cut {
        out.push_str(&if crate::lang::is_en() { format!("# (more than {MAX_ROWS} rows; the rest is omitted)\r\n") } else { format!("# （{MAX_ROWS} 行を超えたため、残りは省略）\r\n") });
    }
    Ok(out)
}

fn safe_name(n: &str) -> String {
    n.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c }).collect()
}

/// （一覧の名前, 中身）の並び。
fn files(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let db = Db::open(b)?;
    let (tables, sqls) = schema(&db)?;
    let mut out = vec![("schema.sql".to_string(), sqls.join("\n\n").into_bytes())];
    for t in &tables {
        let body = table_csv(&db, t).unwrap_or_else(|e| format!("# {e}\r\n"));
        out.push((format!("{}.csv", safe_name(&t.name)), body.into_bytes()));
    }
    Ok(out)
}

fn read_all(p: &Path) -> Result<Vec<u8>, String> {
    if std::fs::metadata(p).map_err(|e| e.to_string())?.len() > MAX_FILE {
        return err("データベースが大きすぎます（512 MiB まで）", "Database too large (up to 512 MiB)");
    }
    std::fs::read(p).map_err(|e| e.to_string())
}

pub fn list_bytes(b: &[u8]) -> Result<Vec<Entry>, String> {
    Ok(files(b)?.into_iter().map(|(n, d)| Entry { path: n, is_dir: false, size: d.len() as u64, packed: None }).collect())
}

pub fn list_file(p: &Path) -> Result<Vec<Entry>, String> {
    list_bytes(&read_all(p)?)
}

pub fn read_path(p: &Path, path: &str, max: u64) -> Result<Vec<u8>, String> {
    let (_, d) = files(&read_all(p)?)?.into_iter().find(|(n, _)| n == path).ok_or_else(|| crate::lang::tr("見つかりません", "Not found").to_string())?;
    if d.len() as u64 > max {
        return err("大きすぎます", "Too large");
    }
    Ok(d)
}

pub fn each_file(p: &Path, max_each: u64, mut f: impl FnMut(&str, Vec<u8>) -> bool) -> Result<(), String> {
    for (n, d) in files(&read_all(p)?)?.into_iter().filter(|(_, d)| d.len() as u64 <= max_each) {
        if !f(&n, d) {
            break;
        }
    }
    Ok(())
}

pub fn extract_to_temp(image: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    super::extfs::write_temp(image, path, &read_path(image, path, super::extfs::EXTRACT_MAX)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_and_columns() {
        assert_eq!(varint(&[0x7F], 0), Some((127, 1)));
        assert_eq!(varint(&[0x81, 0x00], 0), Some((128, 2)));
        assert_eq!(varint(&[0xFF; 9], 0), Some((u64::MAX, 9)));
        let (c, a) = columns("CREATE TABLE t (id INTEGER PRIMARY KEY, \"full name\" TEXT, n REAL DEFAULT (1,2), UNIQUE(n))");
        assert_eq!(c, ["id", "full name", "n"]);
        assert_eq!(a, Some(0));
        assert_eq!(csv_cell(&Value::Text("a,\"b\"".into())), "\"a,\"\"b\"\"\"");
    }

    /// Python の sqlite3 で作った実物（ページ 1 KiB・500 行余り・はみ出す長い文字・後から足した列・WITHOUT ROWID の表・索引）。
    fn sample() -> Vec<u8> {
        crate::neo::deflate::gunzip(include_bytes!("testdata/sample.sqlite.gz"), 1 << 20).unwrap()
    }

    #[test]
    fn reads_a_real_database() {
        let b = sample();
        let names: Vec<String> = list_bytes(&b).unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(names, ["schema.sql", "items.csv"], "WITHOUT ROWID の表は中身を出さない");
        let f = files(&b).unwrap();
        let schema = String::from_utf8(f[0].1.clone()).unwrap();
        assert!(schema.contains("CREATE TABLE kv") && schema.contains("CREATE INDEX ix"));
        let csv = String::from_utf8(f[1].1.clone()).unwrap();
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[0], "id,name,price,data,note");
        assert_eq!(lines[1], "1,品物1,1.5,x'0101',");
        assert_eq!(lines[2], "2,品物2,3,x'0201',\"a,\"\"b\"\"\"");
        assert_eq!(lines[500], "500,品物500,750,x'F401',");
        assert_eq!(lines[501], format!("501,{},,,", "長".repeat(3000)), "はみ出した長い文字も続きのページから読む");
        assert_eq!(lines.len(), 503, "見出し＋501 行＋最後の空");
        for cut in (0..b.len()).step_by(997) {
            let _ = files(&b[..cut]);
        }
    }

    #[test]
    fn broken_input_does_not_panic() {
        assert!(files(b"").is_err());
        let mut b = MAGIC.to_vec();
        b.resize(100, 0);
        assert!(files(&b).is_err());
    }
}
