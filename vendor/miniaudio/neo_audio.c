/* NeoNote の簡易再生用の、miniaudio の薄い包み。
 * デコーダー（MP3 / FLAC / WAV）とデバイス（再生）だけを使う。Rust からは下の関数だけを呼ぶ。
 * 音は miniaudio のデバイスのコールバック（別スレッド）で鳴らす。UI スレッドは止めない。 */
#define MINIAUDIO_IMPLEMENTATION
#define MA_NO_ENCODING
#define MA_NO_GENERATION
#define MA_NO_ENGINE
#define MA_NO_NODE_GRAPH
#define MA_NO_RESOURCE_MANAGER
#include "miniaudio.h"

#include <stdlib.h>
#include <string.h>
#include <stdio.h>

typedef struct neo_audio {
    ma_decoder decoder;
    ma_device device;
    int device_ready;
    int running;           /* ma_device_start 済みで、まだ止めていない（UI スレッドだけが読み書きする） */
    ma_spinlock lock;      /* played / done を、コールバックと UI スレッドで受け渡す */
    ma_uint64 played;      /* 鳴らしたフレーム数 */
    int done;              /* 最後まで読み終えた */
} neo_audio;

static void set_err(char* err, int errlen, const char* what, ma_result r) {
    if (err && errlen > 0) {
        snprintf(err, (size_t)errlen, "%s: %s", what, ma_result_description(r));
    }
}

static void on_data(ma_device* dev, void* out, const void* in, ma_uint32 frames) {
    neo_audio* a = (neo_audio*)dev->pUserData;
    ma_uint64 got = 0;
    ma_result r;
    (void)in;
    r = ma_decoder_read_pcm_frames(&a->decoder, out, frames, &got);
    if (got < frames) {
        ma_uint32 ch = a->decoder.outputChannels;
        memset((float*)out + got * ch, 0, (size_t)(frames - got) * ch * sizeof(float));
    }
    ma_spinlock_lock(&a->lock);
    a->played += got;
    if (got < frames || r == MA_AT_END) {
        a->done = 1;
    }
    ma_spinlock_unlock(&a->lock);
}

static neo_audio* finish_open(neo_audio* a, ma_result r, char* err, int errlen) {
    ma_device_config cfg;
    if (r != MA_SUCCESS) {
        set_err(err, errlen, "音声を読めません", r);
        free(a);
        return NULL;
    }
    cfg = ma_device_config_init(ma_device_type_playback);
    cfg.playback.format = ma_format_f32;
    cfg.playback.channels = a->decoder.outputChannels;
    cfg.sampleRate = a->decoder.outputSampleRate;
    cfg.dataCallback = on_data;
    cfg.pUserData = a;
    r = ma_device_init(NULL, &cfg, &a->device);
    if (r != MA_SUCCESS) {
        set_err(err, errlen, "出力デバイスが開けません", r);
        ma_decoder_uninit(&a->decoder);
        free(a);
        return NULL;
    }
    a->device_ready = 1;
    return a;
}

static neo_audio* alloc_audio(void) {
    neo_audio* a = (neo_audio*)calloc(1, sizeof(neo_audio));
    return a;
}

static ma_decoder_config dec_config(void) {
    return ma_decoder_config_init(ma_format_f32, 0, 0); /* 形式はファイルのまま（チャンネル数・周波数） */
}

#ifdef _WIN32
neo_audio* neo_audio_open_w(const wchar_t* path, char* err, int errlen) {
    ma_decoder_config dc = dec_config();
    neo_audio* a = alloc_audio();
    if (!a) return NULL;
    return finish_open(a, ma_decoder_init_file_w(path, &dc, &a->decoder), err, errlen);
}
#else
neo_audio* neo_audio_open(const char* path, char* err, int errlen) {
    ma_decoder_config dc = dec_config();
    neo_audio* a = alloc_audio();
    if (!a) return NULL;
    return finish_open(a, ma_decoder_init_file(path, &dc, &a->decoder), err, errlen);
}
#endif

/* 再生を始める（止めていた位置から。最後まで鳴らし終えていたら、先頭から）。成功で 0。 */
int neo_audio_start(neo_audio* a, char* err, int errlen) {
    ma_result r;
    int done;
    if (a->running) return 0;
    ma_spinlock_lock(&a->lock);
    done = a->done;
    ma_spinlock_unlock(&a->lock);
    if (done) {
        r = ma_decoder_seek_to_pcm_frame(&a->decoder, 0);
        if (r != MA_SUCCESS) {
            set_err(err, errlen, "先頭へ戻れません", r);
            return -1;
        }
        ma_spinlock_lock(&a->lock);
        a->played = 0;
        a->done = 0;
        ma_spinlock_unlock(&a->lock);
    }
    r = ma_device_start(&a->device);
    if (r != MA_SUCCESS) {
        set_err(err, errlen, "再生を始められません", r);
        return -1;
    }
    a->running = 1;
    return 0;
}

/* 止める（位置は残る）。コールバックが終わるまで待つ。 */
void neo_audio_stop(neo_audio* a) {
    if (!a->running) return;
    ma_device_stop(&a->device);
    a->running = 0;
}

/* 状態: 1 = 再生中, 0 = 止まっている, 2 = 最後まで鳴らし終えた（デバイスはまだ動いている。stop を呼ぶ）。
 * pos_sec に再生位置（秒）を入れる。 */
int neo_audio_poll(neo_audio* a, double* pos_sec) {
    ma_uint64 played;
    int done;
    ma_spinlock_lock(&a->lock);
    played = a->played;
    done = a->done;
    ma_spinlock_unlock(&a->lock);
    if (pos_sec) {
        ma_uint32 rate = a->decoder.outputSampleRate;
        *pos_sec = rate ? (double)played / (double)rate : 0.0;
    }
    if (done) return 2;
    return a->running ? 1 : 0;
}

void neo_audio_close(neo_audio* a) {
    if (!a) return;
    if (a->device_ready) ma_device_uninit(&a->device); /* 動いていれば止めてから解放する */
    ma_decoder_uninit(&a->decoder);
    free(a);
}
