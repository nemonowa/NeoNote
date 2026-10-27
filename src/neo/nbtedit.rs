// NBT の木（Preview の階層表示）で行う編集。GUI なし・依存なし。
// 木の項目は根からの道筋（Seg の並び）で指す。編集した結果は SNBT の文字に戻して本文を置き換える（呼び出し側）。
use super::nbt::Tag;

/// 道筋の 1 段。複合タグは名前、リスト・配列は番号。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Seg {
    Key(String),
    Index(usize),
}

/// 追加するときに選べる型（表示名, タグの番号）。
pub const TYPES: [(&str, u8); 12] = [
    ("Byte", 1),
    ("Short", 2),
    ("Int", 3),
    ("Long", 4),
    ("Float", 5),
    ("Double", 6),
    ("String", 8),
    ("Compound", 10),
    ("List", 9),
    ("Byte Array", 7),
    ("Int Array", 11),
    ("Long Array", 12),
];

pub fn type_name(id: u8) -> &'static str {
    TYPES.iter().find(|t| t.1 == id).map_or("End", |t| t.0)
}

/// その型の空の値（追加したときの初めの値）。
pub fn default_of(id: u8) -> Option<Tag> {
    Some(match id {
        1 => Tag::Byte(0),
        2 => Tag::Short(0),
        3 => Tag::Int(0),
        4 => Tag::Long(0),
        5 => Tag::Float(0.0),
        6 => Tag::Double(0.0),
        7 => Tag::ByteArray(Vec::new()),
        8 => Tag::String(String::new()),
        9 => Tag::List(0, Vec::new()),
        10 => Tag::Compound(Vec::new()),
        11 => Tag::IntArray(Vec::new()),
        12 => Tag::LongArray(Vec::new()),
        _ => return None,
    })
}

pub fn get<'a>(root: &'a Tag, path: &[Seg]) -> Option<&'a Tag> {
    let mut t = root;
    for s in path {
        t = match (t, s) {
            (Tag::Compound(v), Seg::Key(k)) => &v.iter().find(|(n, _)| n == k)?.1,
            (Tag::List(_, v), Seg::Index(i)) => v.get(*i)?,
            _ => return None,
        };
    }
    Some(t)
}

pub fn get_mut<'a>(root: &'a mut Tag, path: &[Seg]) -> Option<&'a mut Tag> {
    let mut t = root;
    for s in path {
        t = match (t, s) {
            (Tag::Compound(v), Seg::Key(k)) => &mut v.iter_mut().find(|(n, _)| n == k)?.1,
            (Tag::List(_, v), Seg::Index(i)) => v.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(t)
}

/// 木の 1 行の文字（名前: 値の短い表し）。
pub fn summary(name: &str, t: &Tag) -> String {
    let short = |s: String| if s.chars().count() > 80 { format!("{}…", s.chars().take(80).collect::<String>()) } else { s };
    let value = match t {
        Tag::Compound(v) => format!("{{{} 個}}", v.len()),
        Tag::List(id, v) => format!("[{} 個 {}]", v.len(), type_name(if v.is_empty() { *id } else { v[0].id() })),
        Tag::ByteArray(v) => format!("[B; {} 個]", v.len()),
        Tag::IntArray(v) => format!("[I; {} 個]", v.len()),
        Tag::LongArray(v) => format!("[L; {} 個]", v.len()),
        Tag::String(s) => short(format!("\"{s}\"")),
        other => super::nbt::to_snbt(other).trim_end().to_string(),
    };
    if name.is_empty() { value } else { format!("{name}: {value}") }
}

/// 値を書き換えるときの入力欄の初めの文字。複合タグ・リストは値として書き換えない（None）。
pub fn edit_text(t: &Tag) -> Option<String> {
    let join = |v: Vec<String>| v.join(", ");
    Some(match t {
        Tag::Byte(v) => v.to_string(),
        Tag::Short(v) => v.to_string(),
        Tag::Int(v) => v.to_string(),
        Tag::Long(v) => v.to_string(),
        Tag::Float(v) => v.to_string(),
        Tag::Double(v) => v.to_string(),
        Tag::String(s) => s.clone(),
        Tag::ByteArray(v) => join(v.iter().map(|x| x.to_string()).collect()),
        Tag::IntArray(v) => join(v.iter().map(|x| x.to_string()).collect()),
        Tag::LongArray(v) => join(v.iter().map(|x| x.to_string()).collect()),
        Tag::List(..) | Tag::Compound(_) => return None,
    })
}

/// 入力を、元と同じ型の値にする（型は変えない。読めなければ理由）。
pub fn parse_edit(old: &Tag, input: &str) -> Result<Tag, String> {
    let s = input.trim();
    fn num<T: std::str::FromStr>(s: &str, what: &str) -> Result<T, String> {
        s.parse::<T>().map_err(|_| format!("{what} として読めません: {s}"))
    }
    fn list<T: std::str::FromStr>(s: &str, what: &str) -> Result<Vec<T>, String> {
        s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(|x| num::<T>(x, what)).collect()
    }
    Ok(match old {
        Tag::Byte(_) => Tag::Byte(match s {
            "true" => 1,
            "false" => 0,
            _ => num(s, "Byte（-128〜127）")?,
        }),
        Tag::Short(_) => Tag::Short(num(s, "Short（-32768〜32767）")?),
        Tag::Int(_) => Tag::Int(num(s, "Int")?),
        Tag::Long(_) => Tag::Long(num(s, "Long")?),
        Tag::Float(_) => Tag::Float(num(s, "Float")?),
        Tag::Double(_) => Tag::Double(num(s, "Double")?),
        Tag::String(_) => Tag::String(input.to_string()),
        Tag::ByteArray(_) => Tag::ByteArray(list(s, "Byte（-128〜127）")?),
        Tag::IntArray(_) => Tag::IntArray(list(s, "Int")?),
        Tag::LongArray(_) => Tag::LongArray(list(s, "Long")?),
        Tag::List(..) | Tag::Compound(_) => return Err("リストと複合タグは、中の項目を書き換えてください".into()),
    })
}

/// 道筋の項目を消す（根は消せない）。
pub fn remove(root: &mut Tag, path: &[Seg]) -> bool {
    let Some((last, parent)) = path.split_last() else { return false };
    match (get_mut(root, parent), last) {
        (Some(Tag::Compound(v)), Seg::Key(k)) => {
            let n = v.len();
            v.retain(|(x, _)| x != k);
            v.len() != n
        }
        (Some(Tag::List(_, v)), Seg::Index(i)) if *i < v.len() => {
            v.remove(*i);
            true
        }
        _ => false,
    }
}

/// 複合タグの中の項目の名前を変える。同じ名前がもうあれば理由を返す。
pub fn rename(root: &mut Tag, path: &[Seg], new_name: &str) -> Result<(), String> {
    let Some((Seg::Key(old), parent)) = path.split_last() else { return Err("名前を変えられるのは複合タグの中の項目だけです".into()) };
    let Some(Tag::Compound(v)) = get_mut(root, parent) else { return Err("項目が見つかりません".into()) };
    if new_name.is_empty() {
        return Err("名前が空です".into());
    }
    if new_name != old && v.iter().any(|(n, _)| n == new_name) {
        return Err(format!("「{new_name}」はもうあります"));
    }
    match v.iter_mut().find(|(n, _)| n == old) {
        Some(e) => {
            e.0 = new_name.to_string();
            Ok(())
        }
        None => Err("項目が見つかりません".into()),
    }
}

/// 複合タグ・リストの末尾に子を足し、足した子の道筋を返す。リストは要素の型がそろっていなければ理由。
pub fn add_child(root: &mut Tag, parent: &[Seg], name: &str, child: Tag) -> Result<Vec<Seg>, String> {
    let mut path = parent.to_vec();
    match get_mut(root, parent) {
        Some(Tag::Compound(v)) => {
            if name.is_empty() {
                return Err("名前が空です".into());
            }
            if v.iter().any(|(n, _)| n == name) {
                return Err(format!("「{name}」はもうあります"));
            }
            v.push((name.to_string(), child));
            path.push(Seg::Key(name.to_string()));
        }
        Some(Tag::List(id, v)) => {
            if let Some(first) = v.first() {
                if first.id() != child.id() {
                    return Err(format!("このリストには {} しか入れられません", type_name(first.id())));
                }
            }
            *id = child.id();
            v.push(child);
            path.push(Seg::Index(v.len() - 1));
        }
        _ => return Err("子を足せるのは複合タグとリストだけです".into()),
    }
    Ok(path)
}

/// 子を足すときに型を選ばせるか。要素のあるリストは、その要素の型に決まる（Some）。
pub fn fixed_child_type(parent: &Tag) -> Option<u8> {
    match parent {
        Tag::List(_, v) if !v.is_empty() => Some(v[0].id()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> Tag {
        Tag::Compound(vec![
            ("Data".into(), Tag::Compound(vec![("Difficulty".into(), Tag::Byte(2)), ("Name".into(), Tag::String("w".into()))])),
            ("List".into(), Tag::List(3, vec![Tag::Int(1), Tag::Int(2)])),
        ])
    }

    fn k(s: &str) -> Seg {
        Seg::Key(s.into())
    }

    #[test]
    fn paths_reach_items_and_edits_keep_the_type() {
        let mut r = root();
        assert_eq!(get(&r, &[k("Data"), k("Difficulty")]), Some(&Tag::Byte(2)));
        assert_eq!(get(&r, &[k("List"), Seg::Index(1)]), Some(&Tag::Int(2)));
        assert_eq!(get(&r, &[k("Nope")]), None);
        let t = get_mut(&mut r, &[k("Data"), k("Difficulty")]).unwrap();
        *t = parse_edit(t, " 3 ").unwrap();
        assert_eq!(get(&r, &[k("Data"), k("Difficulty")]), Some(&Tag::Byte(3)));
        assert!(parse_edit(&Tag::Byte(0), "300").is_err());
        assert_eq!(parse_edit(&Tag::Byte(0), "true").unwrap(), Tag::Byte(1));
        assert_eq!(parse_edit(&Tag::String(String::new()), " a b ").unwrap(), Tag::String(" a b ".into()));
        assert_eq!(parse_edit(&Tag::IntArray(vec![]), "1, -2,3").unwrap(), Tag::IntArray(vec![1, -2, 3]));
        assert!(parse_edit(&Tag::Compound(vec![]), "x").is_err());
        assert_eq!(edit_text(&Tag::LongArray(vec![1, 2])).as_deref(), Some("1, 2"));
        assert_eq!(edit_text(&Tag::List(0, vec![])), None);
    }

    #[test]
    fn rename_remove_and_add() {
        let mut r = root();
        assert!(rename(&mut r, &[k("Data"), k("Name")], "Difficulty").is_err());
        rename(&mut r, &[k("Data"), k("Name")], "LevelName").unwrap();
        assert!(get(&r, &[k("Data"), k("LevelName")]).is_some());
        assert!(rename(&mut r, &[k("List"), Seg::Index(0)], "x").is_err());

        let p = add_child(&mut r, &[k("List")], "", Tag::Int(9)).unwrap();
        assert_eq!(p, vec![k("List"), Seg::Index(2)]);
        assert!(add_child(&mut r, &[k("List")], "", Tag::Byte(1)).is_err());
        assert_eq!(fixed_child_type(get(&r, &[k("List")]).unwrap()), Some(3));
        let p = add_child(&mut r, &[k("Data")], "New", default_of(10).unwrap()).unwrap();
        assert_eq!(get(&r, &p), Some(&Tag::Compound(vec![])));
        assert!(add_child(&mut r, &[k("Data")], "New", Tag::Int(0)).is_err());

        assert!(remove(&mut r, &[k("List"), Seg::Index(0)]));
        assert_eq!(get(&r, &[k("List")]), Some(&Tag::List(3, vec![Tag::Int(2), Tag::Int(9)])));
        assert!(remove(&mut r, &[k("Data")]));
        assert!(!remove(&mut r, &[]));
    }

    #[test]
    fn summaries_show_name_type_and_size() {
        assert_eq!(summary("Difficulty", &Tag::Byte(2)), "Difficulty: 2b");
        assert_eq!(summary("Pos", &Tag::List(6, vec![Tag::Double(1.0)])), "Pos: [1 個 Double]");
        assert_eq!(summary("", &Tag::Compound(vec![])), "{0 個}");
        assert!(summary("s", &Tag::String("x".repeat(200))).ends_with("…"));
    }
}
