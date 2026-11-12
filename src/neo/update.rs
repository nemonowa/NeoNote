// 新しい版の確認（GUI なし）。GitHub の「最新のリリース」の JSON から版の名前と配布ページを読み、いまの版と比べる。
// 下書き・プレリリースは GitHub が「最新」に含めないので、公開した版だけが対象になる。

/// GitHub のリポジトリ（所有者/名前）。
pub const REPO: &str = "nemonowa/NeoNote";

/// 最新のリリースを返す API。
pub fn api_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// リリースの一覧のページ（確認に失敗したときに開く）。
pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases")
}

#[derive(Debug, PartialEq, Eq)]
pub struct Latest {
    /// タグの名前（`v2.1.78` など）。
    pub tag: String,
    /// そのリリースのページ。
    pub page: String,
}

/// API の JSON から、タグと配布ページを読む。リリースが無い・形が違うときは None。
pub fn parse_latest(json: &str) -> Option<Latest> {
    let tag = string_field(json, "tag_name")?;
    // `html_url` は作者などにもあるので、リリース自身のもの（/releases/tag/ を含む）を探す
    let page = all_string_fields(json, "html_url").into_iter().find(|u| u.contains("/releases/tag/")).unwrap_or_else(releases_page);
    Some(Latest { tag, page })
}

fn string_field(json: &str, key: &str) -> Option<String> {
    all_string_fields(json, key).into_iter().next()
}

/// `"key": "値"` の値を、出てくる順にすべて（エスケープは `\"` `\\` `\/` だけ戻す。URL とタグにはそれで足りる）。
fn all_string_fields(json: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{key}\"");
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(p) = rest.find(&pat) {
        rest = &rest[p + pat.len()..];
        let r = rest.trim_start();
        let Some(r) = r.strip_prefix(':') else { continue };
        let Some(r) = r.trim_start().strip_prefix('"') else { continue };
        let mut v = String::new();
        let mut chars = r.chars();
        let mut closed = false;
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    closed = true;
                    break;
                }
                '\\' => match chars.next() {
                    Some(e @ ('"' | '\\' | '/')) => v.push(e),
                    Some(e) => {
                        v.push('\\');
                        v.push(e);
                    }
                    None => break,
                },
                c => v.push(c),
            }
        }
        if closed {
            out.push(v);
        }
    }
    out
}

/// 版の数字（`v2.1.78` → [2, 1, 78]）。数字で始まらない部分（`-beta` など）は無視する。
fn numbers(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map_while(|p| p.parse::<u64>().ok())
        .collect()
}

/// `latest` が `current` より新しいか。
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (a, b) = (numbers(latest), numbers(current));
    if a.is_empty() {
        return false;
    }
    let n = a.len().max(b.len());
    let get = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    for i in 0..n {
        match get(&a, i).cmp(&get(&b, i)) {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tag_and_release_page() {
        let j = r#"{"url":"https://api.github.com/x","html_url":"https:\/\/github.com\/nemonowa\/NeoNote\/releases\/tag\/v2.1.78","tag_name":"v2.1.78",
            "author":{"html_url":"https://github.com/nemonowa"}}"#;
        assert_eq!(parse_latest(j), Some(Latest { tag: "v2.1.78".into(), page: "https://github.com/nemonowa/NeoNote/releases/tag/v2.1.78".into() }));
        let j2 = r#"{"author":{"html_url":"https://github.com/nemonowa"},"tag_name" : "v3"}"#;
        assert_eq!(parse_latest(j2).unwrap().page, releases_page());
        assert_eq!(parse_latest(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_latest(r#"{"tag_name":"v1"#), None); // 途中で切れた
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v2.1.78", "v2.1.77"));
        assert!(is_newer("v2.2", "v2.1.99"));
        assert!(is_newer("2.10.0", "v2.9.9"));
        assert!(!is_newer("v2.1.77", "v2.1.77"));
        assert!(!is_newer("v2.1.76", "v2.1.77"));
        assert!(!is_newer("v2.1.77", "v999.99.9")); // 開発版より新しいものは無い
        assert!(!is_newer("latest", "v2.1.77"));
    }
}
