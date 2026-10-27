// NBT の Preview: NBT Explorer のような階層の木。項目は「名前: 値」で、開閉できる。
// 木は表示だけを受け持ち、編集の操作（ダブルクリック・右クリック・キー）は呼び出し側が受けて、本文（SNBT）を書き換えてから作り直す。
use super::theme::*;
use crate::core::{
    nbt::Tag,
    nbtedit::{self, Seg},
};
use fltk::{
    enums::{Color, Font, FrameType},
    prelude::*,
    tree::{Tree, TreeConnectorStyle, TreeItem, TreeSelect},
};
use std::{cell::RefCell, collections::HashSet, rc::Rc};

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
        let t = NbtTree { tree, items: Rc::new(RefCell::new(Vec::new())) };
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
                if let Some(mut it) = tree.insert(&top, &format!("…（項目が {MAX_ITEMS} を超えたため、残りは省略。Text で編集してください）"), n) {
                    it.set_label_fgcolor(rgb(color_subtext()));
                }
            }
        }
        for (it, p) in items.iter_mut() {
            if it.has_children() {
                let want = match open {
                    Some(set) => set.contains(p),
                    None => p.len() == 1,
                };
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

    /// 木の代わりに、1 行の知らせ（SNBT の書き間違いなど）を出す。
    pub fn show_message(&self, msg: &str) {
        let mut tree = self.tree.clone();
        if let Some(top) = tree.root() {
            tree.clear_children(&top);
        }
        self.items.borrow_mut().clear();
        if let Some(top) = tree.root() {
            if let Some(mut it) = tree.insert(&top, msg, 0) {
                it.set_label_fgcolor(rgb(color_error()));
                it.set_label_bgcolor(rgb(color_main()));
            }
        }
        tree.redraw();
    }
}
