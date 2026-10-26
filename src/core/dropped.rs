// ドロップされたテキスト（file:// URI や素のパスが改行区切りで入る）をパスに直す。
use std::path::PathBuf;

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn parse_dropped_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| match l.strip_prefix("file://") {
            Some(rest) => {
                let rest = rest.strip_prefix("localhost").unwrap_or(rest);
                let mut p = percent_decode(rest);
                // Windows: file:///C:/dir → /C:/dir → C:/dir
                let b = p.as_bytes();
                if b.len() >= 3 && b[0] == b'/' && b[2] == b':' {
                    p.remove(0);
                }
                PathBuf::from(p)
            }
            None => PathBuf::from(l),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_and_plain_paths() {
        let t = "file:///home/u/My%20Docs/%E3%83%A1%E3%83%A2.txt\r\n/plain/path.md\n\n";
        let v = parse_dropped_paths(t);
        assert_eq!(v, vec![PathBuf::from("/home/u/My Docs/メモ.txt"), PathBuf::from("/plain/path.md")]);
    }

    #[test]
    fn windows_drive_uri() {
        assert_eq!(parse_dropped_paths("file:///C:/a/b.txt"), vec![PathBuf::from("C:/a/b.txt")]);
    }

    #[test]
    fn stray_percent_is_kept() {
        assert_eq!(parse_dropped_paths("/a/100%.txt"), vec![PathBuf::from("/a/100%.txt")]);
        assert_eq!(parse_dropped_paths("/a/%"), vec![PathBuf::from("/a/%")]);
    }
}
