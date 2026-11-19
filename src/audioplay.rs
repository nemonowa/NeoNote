// 音声の簡易再生（再生・停止・位置）。miniaudio の薄い包み（vendor/miniaudio/neo_audio.c）を呼ぶ。
// `core` には入れない（GUI なし・テスト可能な層に、音の出力を持ち込まない）。UI スレッドだけから使う。
// 鳴らすのは miniaudio のデバイスのコールバック（別スレッド）。Drop でデバイスを止めて解放する。
// `audio` feature を外したビルドでは、開く時点で「再生は含まれていません」のエラーを返す。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Stopped,
    Playing,
    /// 最後まで鳴らし終えた（`stop` を呼ぶとデバイスを止める。もう一度 `play` すると先頭から）。
    Finished,
}

#[cfg(feature = "audio")]
mod imp {
    use super::State;
    use std::ffi::{c_char, c_int, c_void};
    use std::path::Path;

    extern "C" {
        #[cfg(not(windows))]
        fn neo_audio_open(path: *const c_char, err: *mut c_char, errlen: c_int) -> *mut c_void;
        #[cfg(windows)]
        fn neo_audio_open_w(path: *const u16, err: *mut c_char, errlen: c_int) -> *mut c_void;
        fn neo_audio_start(a: *mut c_void, err: *mut c_char, errlen: c_int) -> c_int;
        fn neo_audio_stop(a: *mut c_void);
        fn neo_audio_poll(a: *mut c_void, pos_sec: *mut f64) -> c_int;
        fn neo_audio_close(a: *mut c_void);
        fn neo_audio_length(a: *mut c_void) -> f64;
        fn neo_audio_seek(a: *mut c_void, sec: f64) -> c_int;
        fn neo_audio_volume(a: *mut c_void, v: f64);
    }

    pub struct Player {
        p: *mut c_void,
    }

    fn err_text(buf: &[c_char]) -> String {
        let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    impl Player {
        /// ファイルを開く（デコーダーと出力デバイスを用意する。まだ鳴らさない）。
        pub fn open(path: &Path) -> Result<Player, String> {
            let mut err = [0 as c_char; 256];
            #[cfg(not(windows))]
            let p = {
                use std::os::unix::ffi::OsStrExt;
                let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| crate::lang::tr("パスに NUL が含まれています", "path contains a NUL character").to_string())?;
                unsafe { neo_audio_open(c.as_ptr(), err.as_mut_ptr(), err.len() as c_int) }
            };
            #[cfg(windows)]
            let p = {
                use std::os::windows::ffi::OsStrExt;
                let w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                unsafe { neo_audio_open_w(w.as_ptr(), err.as_mut_ptr(), err.len() as c_int) }
            };
            if p.is_null() {
                let e = err_text(&err);
                return Err(if e.is_empty() { crate::lang::tr("音声を開けません", "Cannot open the audio").into() } else { e });
            }
            Ok(Player { p })
        }

        pub fn play(&mut self) -> Result<(), String> {
            let mut err = [0 as c_char; 256];
            if unsafe { neo_audio_start(self.p, err.as_mut_ptr(), err.len() as c_int) } == 0 {
                Ok(())
            } else {
                Err(err_text(&err))
            }
        }

        pub fn stop(&mut self) {
            unsafe { neo_audio_stop(self.p) }
        }

        /// 状態と再生位置（秒）。
        pub fn poll(&mut self) -> (State, f64) {
            let mut pos = 0.0f64;
            let s = unsafe { neo_audio_poll(self.p, &mut pos) };
            (match s { 1 => State::Playing, 2 => State::Finished, _ => State::Stopped }, pos)
        }

        /// 長さ（秒）。分からなければ 0。
        pub fn duration(&self) -> f64 {
            unsafe { neo_audio_length(self.p) }
        }

        pub fn seek(&mut self, sec: f64) {
            unsafe { neo_audio_seek(self.p, sec) };
        }

        /// 音量（0〜1）。
        pub fn set_volume(&mut self, v: f64) {
            unsafe { neo_audio_volume(self.p, v) }
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            unsafe { neo_audio_close(self.p) }
        }
    }
}

#[cfg(not(feature = "audio"))]
mod imp {
    use super::State;
    use std::path::Path;

    pub struct Player;

    impl Player {
        pub fn open(_: &Path) -> Result<Player, String> {
            Err(crate::lang::tr("このビルドには音声の再生が含まれていません（audio feature なし）", "This build does not include audio playback (no audio feature)").into())
        }
        pub fn play(&mut self) -> Result<(), String> {
            Err(crate::lang::tr("再生は含まれていません", "Playback is not included").into())
        }
        pub fn stop(&mut self) {}
        pub fn poll(&mut self) -> (State, f64) {
            (State::Stopped, 0.0)
        }
        pub fn duration(&self) -> f64 {
            0.0
        }
        pub fn seek(&mut self, _sec: f64) {}
        pub fn set_volume(&mut self, _v: f64) {}
    }
}

pub use imp::Player;
