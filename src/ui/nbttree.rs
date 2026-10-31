// NBT の Preview: NBT Explorer のような階層の木。項目は「名前: 値」で、開閉できる。
// フォルダ・zip の一覧（`show_archive`）も同じ木で出す（項目は「名前  大きさ」。開閉だけで、編集はしない）。
// 木は表示だけを受け持ち、編集の操作（ダブルクリック・右クリック・キー）は呼び出し側が受けて、本文（SNBT）を書き換えてから作り直す。
use super::theme::*;
use crate::neo::{
    archive,
    nbt::Tag,
    nbtedit::{self, Seg},
};
use fltk::{
    enums::{Color, Font, FrameType},
    prelude::*,
    tree::{Tree, TreeConnectorStyle, TreeItem, TreeSelect},
};
use std::{cell::{Cell, RefCell}, collections::HashSet, rc::Rc};

/// 木に入れる項目の上限（大きなチャンクでも作り直しを軽く保つ。超えた分は Text で編集する）。
const MAX_ITEMS: usize = 20_000;

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

#[derive(Clone)]
pub struct NbtTree {
    pub tree: Tree,
    /// 木の項目と、その道筋。
    items: Rc<RefCell<Vec<(TreeItem, Vec<Seg>)>>>,
    /// 検索で絞っている間の、一致した項目の道筋（表示の順）。絞っていなければ空。
    hits: Rc<RefCell<Vec<Vec<Seg>>>>,
    /// 検索で絞っている間は、絞る前の開き方（絞った木は全部開くので、`open_paths` はこちらを返す）。
    filtered: Rc<Cell<bool>>,
    base_open: Rc<RefCell<HashSet<Vec<Seg>>>>,
}

impl NbtTree {
    pub fn new() -> NbtTree {
        let mut tree = Tree::default();
        tree.set_show_root(false);
        tree.set_select_mode(TreeSelect::Single);
        tree.set_connector_style(TreeConnectorStyle::Solid);
        tree.set_select_frame(FrameType::FlatBox);
        tree.set_frame(FrameType::FlatBox);
        tree.set_margin_left(8);
        tree.set_margin_top(8);
        tree.set_line_spacing(4);
        tree.hide();
        let t = NbtTree { tree, items: Rc::new(RefCell::new(Vec::new())), hits: Rc::new(RefCell::new(Vec::new())), filtered: Rc::new(Cell::new(false)), base_open: Rc::new(RefCell::new(HashSet::new())) };
        t.restyle(Font::Helvetica, 14);
        t
    }

    /// 今のパレット・フォント・文字サイズに合わせる。
    pub fn restyle(&self, font: Font, size: i32) {
        let mut tree = self.tree.clone();
        tree.set_color(rgb(color_main()));
        tree.set_selection_color(rgb(color_accent()));
        tree.set_item_label_fgcolor(rgb(color_text()));
        tree.set_item_label_bgcolor(rgb(color_main()));
        tree.set_connector_color(rgb(color_subtext()));
        tree.set_item_label_font(font);
        tree.set_item_label_size(size);
        for (it, _) in self.items.borrow_mut().iter_mut() {
            it.set_label_fgcolor(rgb(color_text()));
            it.set_label_bgcolor(rgb(color_main()));
            it.set_label_font(font);
            it.set_label_size(size);
        }
        super::skin::style_scrollbars(&tree);
        tree.redraw();
    }

    /// 開いている項目の道筋（作り直しても開いたままにするため）。
    pub fn open_paths(&self) -> HashSet<Vec<Seg>> {
        if self.filtered.get() {
            return self.base_open.borrow().clone();
        }
        self.items.borrow().iter().filter(|(it, _)| it.has_children() && it.is_open()).map(|(_, p)| p.clone()).collect()
    }

    pub fn path_of(&self, item: &TreeItem) -> Option<Vec<Seg>> {
        self.items.borrow().iter().find(|(it, _)| it == item).map(|(_, p)| p.clone())
    }

    pub fn selected_path(&self) -> Option<Vec<Seg>> {
        self.tree.first_selected_item().and_then(|it| self.path_of(&it))
    }

    /// 木を作り直す。`open` が None なら 1 段目だけ開く。`select` の項目を選んで見える所へ出す。
    pub fn show(&self, root: &Tag, open: Option<&HashSet<Vec<Seg>>>, select: Option<&[Seg]>) {
        self.filtered.set(false);
        self.hits.borrow_mut().clear();
        let mut tree = self.tree.clone();
        // clear() は根の項目まで消して、足す先が無くなる。根の子だけを消す
        if let Some(top) = tree.root() {
            tree.clear_children(&top);
        }
        let mut items = self.items.borrow_mut();
        items.clear();
        let Some(top) = tree.root() else { return };
        let mut stack: Vec<(TreeItem, Vec<Seg>, &Tag)> = vec![(top, Vec::new(), root)];
        let mut cut = false;
        while let Some((parent, path, tag)) = stack.pop() {
            let children: Vec<(Seg, String, &Tag)> = match tag {
                Tag::Compound(v) => v.iter().map(|(k, t)| (Seg::Key(k.clone()), k.clone(), t)).collect(),
                Tag::List(_, v) => v.iter().enumerate().map(|(i, t)| (Seg::Index(i), format!("[{i}]"), t)).collect(),
                _ => Vec::new(),
            };
            for (pos, (seg, name, t)) in children.into_iter().enumerate() {
                if items.len() >= MAX_ITEMS {
                    cut = true;
                    break;
                }
                let Some(mut it) = tree.insert(&parent, &nbtedit::summary(&name, t), pos as i32) else { continue };
                it.set_label_fgcolor(rgb(color_text()));
                it.set_label_bgcolor(rgb(color_main()));
                let mut p = path.clone();
                p.push(seg);
                if matches!(t, Tag::Compound(_) | Tag::List(..)) {
                    stack.push((it.clone(), p.clone(), t));
                }
                items.push((it, p));
            }
        }
        if cut {
            if let Some(top) = tree.root() {
                let n = top.children();
                let note = if crate::lang::is_en() {
                    format!("… (more than {MAX_ITEMS} items, the rest is omitted. Edit in Text)")
                } else {
                    format!("…（項目が {MAX_ITEMS} を超えたため、残りは省略。Text で編集してください）")
                };
                if let Some(mut it) = tree.insert(&top, &note, n) {
                    it.set_label_fgcolor(rgb(color_subtext()));
                }
            }
        }
        for (it, p) in items.iter_mut() {
            if it.has_children() {
                let want = open.is_some_and(|set| set.contains(p));
                if want { it.open() } else { it.close() }
            }
        }
        let sel = select.and_then(|s| items.iter().find(|(_, p)| p.as_slice() == s).map(|(it, _)| it.clone()));
        drop(items);
        if let Some(mut it) = sel {
            // 選んだ項目の親を開いてから選ぶ
            let mut up = it.parent();
            while let Some(mut u) = up {
                u.open();
                up = u.parent();
            }
            let _ = tree.select_only(&it, false);
            tree.show_item_middle(&it);
            it.select(1);
        }
        tree.redraw();
    }

    /// フォルダ・zip の一覧を木にする。`open` が None（開いた直後）なら、フォルダは全て閉じた状態で出す。道筋はフォルダ名を `Seg::Key` で並べた物。
    /// `find` が空でなければ、名前が一致する項目（`content` は本文が一致したファイルのパス）と、その親のフォルダだけを、全て開いて出す（検索バー）。
    pub fn show_archive(&self, title: &str, entries: &[archive::Entry], packed: bool, open: Option<&HashSet<Vec<Seg>>>, find: &str, content: &HashSet<String>) {
        let find = find.trim();
        let mut tree = self.tree.clone();
        if let Some(top) = tree.root() {
            tree.clear_children(&top);
        }
        let mut items = self.items.borrow_mut();
        items.clear();
        let Some(top) = tree.root() else { return };
        // 見出し（名前と合計）。道筋を持たないので、開閉や選択の対象にならない
        if let Some(mut it) = tree.insert(&top, &format!("{title}    {}", archive::summary(entries, packed)), 0) {
            it.set_label_fgcolor(rgb(color_subtext()));
            it.set_label_bgcolor(rgb(color_main()));
        }
        let mut root = archive::build_tree(entries);
        let filtering = !find.is_empty();
        self.filtered.set(filtering);
        let mut hit_paths: Vec<Vec<Seg>> = Vec::new();
        if filtering {
            *self.base_open.borrow_mut() = open.cloned().unwrap_or_default();
            let (t, hits) = archive::filter_tree(&root, find, content);
            root = t;
            hit_paths = hits.into_iter().map(|h| h.into_iter().map(Seg::Key).collect()).collect();
        }
        *self.hits.borrow_mut() = hit_paths;
        let hit_set: HashSet<Vec<Seg>> = self.hits.borrow().iter().cloned().collect();
        let mut stack: Vec<(TreeItem, Vec<Seg>, &archive::Node, i32)> = vec![(top, Vec::new(), &root, 1)];
        let mut cut = false;
        while let Some((parent, path, node, first)) = stack.pop() {
            for (pos, c) in node.children.iter().enumerate() {
                if items.len() >= MAX_ITEMS {
                    cut = true;
                    break;
                }
                let size = if packed { format!("{}  ({})", archive::human_size(c.size), archive::human_size(c.packed)) } else { archive::human_size(c.size) };
                // `/` は木の階層の区切りとして読まれるので、名前の中の `/` は使わない（フォルダは末尾の / を付けない）
                let label = format!("{}    {size}", c.name.replace('/', "\u{2215}"));
                let Some(mut it) = tree.insert(&parent, &label, first + pos as i32) else { continue };
                let mut p = path.clone();
                p.push(Seg::Key(c.name.clone()));
                // 絞っている間は、一致した項目を濃く、一致の親（通り道）を薄く出す
                let strong = if filtering { hit_set.contains(&p) } else { c.is_dir };
                it.set_label_fgcolor(rgb(if strong { color_text() } else { color_subtext() }));
                it.set_label_bgcolor(rgb(color_main()));
                if c.is_dir && !c.children.is_empty() {
                    stack.push((it.clone(), p.clone(), c, 0));
                }
                items.push((it, p));
            }
        }
        if cut {
            if let Some(top) = tree.root() {
                let n = top.children();
                let note = if crate::lang::is_en() { format!("… (more than {MAX_ITEMS} items, the rest is omitted)") } else { format!("…（項目が {MAX_ITEMS} を超えたため、残りは省略）") };
                if let Some(mut it) = tree.insert(&top, &note, n) {
                    it.set_label_fgcolor(rgb(color_subtext()));
                }
            }
        }
        for (it, p) in items.iter_mut() {
            if it.has_children() {
                let want = filtering || open.is_some_and(|set| set.contains(p));
                if want { it.open() } else { it.close() }
            }
        }
        drop(items);
        tree.redraw();
    }

    /// 検索で一致した項目の数。
    pub fn hit_count(&self) -> usize {
        self.hits.borrow().len()
    }

    /// 検索で一致した項目を、次（`forward`）か前へ 1 つ進んで選ぶ（末尾の先は先頭へ戻る）。
    /// いま一致した項目を選んでいればその次から、そうでなければ先頭（前へなら末尾）から。戻り値は (何番目, 全部で)。
    pub fn step_hit(&self, forward: bool) -> Option<(usize, usize)> {
        let hits = self.hits.borrow();
        let n = hits.len();
        if n == 0 {
            return None;
        }
        let at = self.selected_path().and_then(|p| hits.iter().position(|h| *h == p));
        let i = match (at, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        let want = hits[i].clone();
        drop(hits);
        let it = self.items.borrow().iter().find(|(_, p)| *p == want).map(|(it, _)| it.clone())?;
        let mut tree = self.tree.clone();
        let _ = tree.select_only(&it, false);
        tree.show_item_middle(&it);
        tree.redraw();
        Some((i + 1, n))
    }

    /// 道筋 `path` の項目を選んで見える所へ出す（作り直したあとに、選んでいた所へ戻す）。無ければ false。
    pub fn select_path(&self, path: &[Seg]) -> bool {
        let Some(it) = self.items.borrow().iter().find(|(_, p)| p.as_slice() == path).map(|(it, _)| it.clone()) else { return false };
        let mut tree = self.tree.clone();
        let _ = tree.select_only(&it, false);
        tree.show_item_middle(&it);
        tree.redraw();
        true
    }

    /// 木の代わりに、1 行の知らせ（SNBT の書き間違いなど）を出す。
    pub fn show_message(&self, msg: &str) {
        let mut tree = self.tree.clone();
        if let Some(top) = tree.root() {
            tree.clear_children(&top);
        }
        self.items.borrow_mut().clear();
        self.hits.borrow_mut().clear();
        self.filtered.set(false);
        if let Some(top) = tree.root() {
            if let Some(mut it) = tree.insert(&top, msg, 0) {
                it.set_label_fgcolor(rgb(color_error()));
                it.set_label_bgcolor(rgb(color_main()));
            }
        }
        tree.redraw();
    }
}
