// 動画と音声の再生の画面。上に映像（子の窓。描くのは OS）か音声の情報（タグ）、下に再生・一時停止・位置のバー・時間・音量の行。
// 動画と、Windows の音声は winmedia::Player（Media Foundation）。ほかの OS の音声と、Media Foundation で鳴らせない音声
// （拡張の入っていない ogg など）は audioplay（miniaudio）。

use crate::ui::theme::{color_accent, color_main, color_sub, color_text};
use crate::winmedia::Player;
use fltk::{
    app,
    button::Button,
    enums::{Align, Color, FrameType},
    frame::Frame,
    group::Flex,
    prelude::*,
    valuator::HorNiceSlider,
    window::Window,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const BAR_H: i32 = 30;
/// 位置のバーと時間を直す間隔（秒）。
const TICK: f64 = 0.2;

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb(r, g, b)
}

/// 鳴らす仕組み。どちらも同じ操作で扱う。
enum Media {
    Mf(Player),
    Mini(RefCell<crate::audioplay::Player>),
}

impl Media {
    fn mini_state(&self) -> (crate::audioplay::State, f64) {
        match self {
            Media::Mini(p) => p.borrow_mut().poll(),
            Media::Mf(_) => (crate::audioplay::State::Stopped, 0.0),
        }
    }
    fn play(&self) {
        match self {
            Media::Mf(p) => p.play(),
            Media::Mini(p) => {
                let _ = p.borrow_mut().play();
            }
        }
    }
    fn pause(&self) {
        match self {
            Media::Mf(p) => p.pause(),
            Media::Mini(p) => p.borrow_mut().stop(),
        }
    }
    fn is_paused(&self) -> bool {
        match self {
            Media::Mf(p) => p.is_paused(),
            Media::Mini(_) => self.mini_state().0 != crate::audioplay::State::Playing,
        }
    }
    fn is_ended(&self) -> bool {
        match self {
            Media::Mf(p) => p.is_ended(),
            Media::Mini(_) => self.mini_state().0 == crate::audioplay::State::Finished,
        }
    }
    fn time(&self) -> f64 {
        match self {
            Media::Mf(p) => p.time(),
            Media::Mini(_) => self.mini_state().1,
        }
    }
    fn duration(&self) -> f64 {
        match self {
            Media::Mf(p) => p.duration(),
            Media::Mini(p) => p.borrow().duration(),
        }
    }
    fn seek(&self, t: f64) {
        match self {
            Media::Mf(p) => p.seek(t),
            Media::Mini(p) => p.borrow_mut().seek(t),
        }
    }
    fn set_volume(&self, v: f64) {
        match self {
            Media::Mf(p) => p.set_volume(v),
            Media::Mini(p) => p.borrow_mut().set_volume(v),
        }
    }
    fn video_size(&self) -> Option<(u32, u32)> {
        match self {
            Media::Mf(p) => p.video_size(),
            Media::Mini(_) => None,
        }
    }
    fn resize(&self, w: i32, h: i32) {
        if let Media::Mf(p) = self {
            p.resize(w, h);
        }
    }
    fn error(&self) -> bool {
        matches!(self, Media::Mf(p) if p.error())
    }
}

struct State {
    player: Option<Media>,
    /// 音声（映像の代わりに情報を出す。隠しても止めない）
    audio: bool,
    path: Option<PathBuf>,
    ticking: bool,
    shown_size: (i32, i32),
}

#[derive(Clone)]
pub struct VideoView {
    pub group: Flex,
    /// 映像の置き場（黒）。映像の窓は、この中に縦横比を保って真ん中に置く
    stage: fltk::group::Group,
    surface: Window,
    /// 音声の情報（タグ）
    info: fltk::misc::HelpView,
    note: Frame,
    play: Button,
    seek: HorNiceSlider,
    time: Frame,
    vol: HorNiceSlider,
    st: Rc<RefCell<State>>,
}

fn clock(t: f64) -> String {
    let s = t.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl VideoView {
    pub fn new() -> VideoView {
        let mut group = Flex::default().column();
        group.set_pad(0);
        let mut stage = fltk::group::Group::default();
        stage.set_frame(FrameType::FlatBox);
        stage.set_color(Color::Black);
        let mut surface = Window::default();
        surface.set_color(Color::Black);
        surface.end();
        stage.end();
        let mut info = fltk::misc::HelpView::default();
        info.set_frame(FrameType::FlatBox);
        info.set_color(rgb(color_main()));
        info.hide();
        let mut note = Frame::default();
        note.set_label_color(rgb(color_text()));
        note.set_frame(FrameType::FlatBox);
        note.set_color(Color::Black);
        note.hide();
        let mut row = Flex::default().row();
        row.set_pad(6);
        row.set_margin(3);
        let mut play = Button::default().with_label("▶");
        let mut seek = HorNiceSlider::default();
        let mut time = Frame::default().with_label("0:00 / 0:00");
        time.set_align(Align::Center | Align::Inside);
        let mut vol = HorNiceSlider::default();
        row.fixed(&play, 34);
        row.fixed(&time, 120);
        row.fixed(&vol, 90);
        row.end();
        group.fixed(&row, BAR_H);
        group.end();
        group.hide();
        play.set_frame(FrameType::FlatBox);
        play.set_color(rgb(color_sub()));
        play.set_label_color(rgb(color_text()));
        row.set_frame(FrameType::FlatBox);
        row.set_color(rgb(color_sub()));
        time.set_label_color(rgb(color_text()));
        for s in [&mut seek, &mut vol] {
            s.set_color(rgb(color_main()));
            s.set_selection_color(rgb(color_accent()));
            s.set_frame(FrameType::FlatBox);
        }
        seek.set_range(0.0, 1.0);
        vol.set_range(0.0, 1.0);
        vol.set_value(1.0);
        vol.set_tooltip(crate::lang::tr("音量", "Volume"));
        play.set_tooltip(crate::lang::tr("再生 / 一時停止（Space）", "Play / Pause (Space)"));
        let st = Rc::new(RefCell::new(State { player: None, audio: false, path: None, ticking: false, shown_size: (0, 0) }));
        let v = VideoView { group, stage, surface, info, note, play, seek, time, vol, st };
        {
            let v2 = v.clone();
            v.play.clone().set_callback(move |_| v2.toggle());
        }
        {
            let v2 = v.clone();
            v.seek.clone().set_callback(move |s| {
                let st = v2.st.borrow();
                if let Some(p) = &st.player {
                    p.seek(s.value());
                }
            });
        }
        {
            let v2 = v.clone();
            v.vol.clone().set_callback(move |s| {
                if let Some(p) = &v2.st.borrow().player {
                    p.set_volume(s.value());
                }
            });
        }
        {
            // Space: 再生 / 一時停止、← →: 5 秒戻る・進む。映像をクリックしても再生 / 一時停止
            let v2 = v.clone();
            let mut g = v.group.clone();
            g.handle(move |g, ev| match ev {
                fltk::enums::Event::Focus | fltk::enums::Event::Unfocus => true,
                fltk::enums::Event::Push if app::event_inside_widget(&v2.surface) => {
                    let _ = g.take_focus();
                    v2.toggle();
                    true
                }
                fltk::enums::Event::KeyDown if !app::is_event_ctrl() && !app::is_event_alt() && !app::is_event_command() => match app::event_key() {
                    k if k == fltk::enums::Key::from_char(' ') => {
                        v2.toggle();
                        true
                    }
                    fltk::enums::Key::Left => {
                        v2.skip(-5.0);
                        true
                    }
                    fltk::enums::Key::Right => {
                        v2.skip(5.0);
                        true
                    }
                    _ => false,
                },
                _ => false,
            });
        }
        {
            let v2 = v.clone();
            v.stage.clone().resize_callback(move |_, _, _, _, _| v2.fit());
        }
        v
    }

    /// 表示して `path` を読む（同じファイルならそのまま）。読み始めは止まった状態。`audio` なら映像の代わりに情報を出す。
    pub fn open(&self, path: &Path, audio: bool) {
        self.group.clone().show();
        let same = self.st.borrow().path.as_deref() == Some(path);
        if !same {
            self.close();
            let mut stage = self.stage.clone();
            let mut info = self.info.clone();
            let mut surface = self.surface.clone();
            if audio {
                stage.hide();
                info.show();
            } else {
                info.hide();
                stage.show();
                surface.show();
            }
            self.group.clone().layout();
            let hwnd = if audio { std::ptr::null_mut() } else { surface.raw_handle() };
            let mut player = Player::new(hwnd, path).map(Media::Mf);
            if player.is_none() && audio {
                player = Self::mini(path);
            }
            let mut note = self.note.clone();
            if player.is_none() {
                stage.hide();
                note.set_label(if audio { crate::lang::tr("この音声は再生できません", "This audio cannot be played") } else { crate::lang::tr("この動画は再生できません（この OS では、または形式に対応していません）", "This video cannot be played (not supported on this OS or format)") });
                note.show();
            } else {
                note.hide();
            }
            if let Some(p) = &player {
                p.set_volume(self.vol.value());
            }
            let mut st = self.st.borrow_mut();
            st.player = player;
            st.audio = audio;
            st.path = Some(path.to_path_buf());
            st.shown_size = (0, 0);
        }
        self.group.clone().layout();
        let _ = self.group.clone().take_focus();
        self.arm();
    }

    fn mini(path: &Path) -> Option<Media> {
        crate::audioplay::Player::open(path).ok().map(|p| Media::Mini(RefCell::new(p)))
    }

    /// 音声の情報（タグ）の HTML。
    pub fn set_info(&self, html: &str) {
        self.info.clone().set_value(html);
    }

    /// いま鳴らしているか（音声を別のタブで鳴らしたままにしているか、を見るため）。
    pub fn playing(&self) -> bool {
        self.st.borrow().player.as_ref().is_some_and(|p| !p.is_paused() && !p.is_ended())
    }

    /// 隠す。動画は止める（位置は覚えたまま）。音声は鳴らしたままにする（別のタブを見ながら聞ける）。
    pub fn hide(&self) {
        let st = self.st.borrow();
        if let Some(p) = &st.player {
            if !st.audio {
                p.pause();
            }
        }
        drop(st);
        self.group.clone().hide();
    }

    /// 再生をやめて片付ける（別のファイル・タブを閉じた時）。
    pub fn close(&self) {
        let mut st = self.st.borrow_mut();
        st.player = None;
        st.path = None;
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.st.borrow().path.clone()
    }

    pub fn visible(&self) -> bool {
        self.group.visible()
    }

    pub fn toggle(&self) {
        let st = self.st.borrow();
        let Some(p) = &st.player else { return };
        if p.is_paused() || p.is_ended() {
            if p.is_ended() {
                p.seek(0.0);
            }
            p.play();
        } else {
            p.pause();
        }
        drop(st);
        self.refresh();
    }

    /// 前後へ `secs` 秒動かす（矢印キー）。
    pub fn skip(&self, secs: f64) {
        if let Some(p) = &self.st.borrow().player {
            p.seek(p.time() + secs);
        }
        self.refresh();
    }

    fn arm(&self) {
        if self.st.borrow().ticking {
            return;
        }
        self.st.borrow_mut().ticking = true;
        let v = self.clone();
        app::add_timeout3(TICK, move |h| {
            if !v.group.visible() || v.st.borrow().player.is_none() {
                v.st.borrow_mut().ticking = false;
                return;
            }
            v.refresh();
            app::repeat_timeout3(TICK, h);
        });
    }

    /// 映像の窓を、置き場の中に縦横比を保って真ん中に置く（OS は窓いっぱいに描く）。
    fn fit(&self) {
        let st = self.st.borrow();
        let stage = &self.stage;
        let (x, y, w, h) = (stage.x(), stage.y(), stage.w(), stage.h());
        let (dw, dh) = match st.player.as_ref().and_then(|p| p.video_size()) {
            Some((vw, vh)) => {
                let s = (w as f64 / vw as f64).min(h as f64 / vh as f64);
                (((vw as f64 * s) as i32).max(1), ((vh as f64 * s) as i32).max(1))
            }
            None => (w, h),
        };
        let mut surface = self.surface.clone();
        surface.resize(x + (w - dw) / 2, y + (h - dh) / 2, dw, dh);
        if let Some(p) = &st.player {
            p.resize(dw, dh);
        }
        drop(st);
        self.stage.clone().redraw();
    }

    fn refresh(&self) {
        let mut st = self.st.borrow_mut();
        let Some(p) = &st.player else { return };
        if p.error() && st.audio {
            // Media Foundation で鳴らせない音声（拡張の無い ogg など）は miniaudio で鳴らし直す
            let path = st.path.clone();
            let vol = self.vol.value();
            st.player = path.as_deref().and_then(Self::mini);
            if let Some(p) = &st.player {
                p.set_volume(vol);
            }
            return;
        }
        if p.error() {
            drop(st);
            self.stage.clone().hide();
            let mut n = self.note.clone();
            n.set_label(crate::lang::tr("この動画は再生できません（形式に対応していません）", "This video cannot be played (unsupported format)"));
            n.show();
            self.close();
            self.group.clone().layout();
            return;
        }
        let (t, d) = (p.time(), p.duration());
        let paused = p.is_paused() || p.is_ended();
        // 映像の大きさが分かったら（読み込みの後）置き直す
        let known = p.video_size().is_some() as i32;
        let fit_now = st.shown_size.0 != known;
        st.shown_size.0 = known;
        drop(st);
        if fit_now {
            self.fit();
        }
        // バーをつまんでいる間は、再生の位置でバーを動かさない
        let dragging = app::pushed().is_some_and(|w| w.as_widget_ptr() == self.seek.as_widget_ptr());
        let mut seek = self.seek.clone();
        if d > 0.0 {
            seek.set_range(0.0, d);
        }
        if !dragging {
            seek.set_value(t);
        }
        self.time.clone().set_label(&format!("{} / {}", clock(t), clock(d)));
        self.play.clone().set_label(if paused { "▶" } else { "❚❚" });
    }
}
