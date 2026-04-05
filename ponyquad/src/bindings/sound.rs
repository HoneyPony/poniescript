use std::{os::raw::c_void, sync::Mutex};

use macroquad::audio::PlaySoundParams;
use poniescript_gc::{GcContext, Gp, HasPsHeader, HasPsType, PONI_TAG_OPAQUE, PsBool, PsFloat, PsInt, PsObject, ps_bool};
use poniescript_rt::{PsStrBuf, Vec2, Vec4};

#[repr(C)]
pub struct PqSound {
    header: PsObject,
    inner: Option<macroquad::audio::Sound>,
}

unsafe impl HasPsHeader for PqSound {}

impl HasPsType for PqSound {
    const TYP: u64 = PONI_TAG_OPAQUE;
}


/// Handles loading the Sounds "later."
/// 
/// Will need to be visited as a GC root...
struct SoundQueue {
    outstanding: Vec<(Gp<PqSound>, String)>,

    to_play_looping: Vec<Gp<PqSound>>,
}

impl SoundQueue {
    const fn new() -> Self {
        Self {
            outstanding: Vec::new(),
            to_play_looping: Vec::new(),
        }
    }

    fn push(&mut self, path: Gp<PsStrBuf>, result: Gp<PqSound>) {
        self.outstanding.push((result, path.get_inner().get_string().into_owned()));
    }

    fn push_looper(&mut self, sound: Gp<PqSound>) {
        self.to_play_looping.push(sound);
    }
}

static SOUND_QUEUE: Mutex<SoundQueue> = Mutex::new(SoundQueue::new()); 

pub async fn process_queue() {
    let queue = {
        let mut queue = SOUND_QUEUE.lock().unwrap();
        std::mem::take(&mut queue.outstanding)
    };

    for (sound, path) in queue {
        let inner = macroquad::audio::load_sound(&path).await;
        if let Ok(inner) = inner {
            // SAFETY: We don't support multithreading, so it is not possible
            // for another thread to interfere.
            //
            // If it was, we could add an additional level of indirection to make
            // it safe.
            unsafe { sound.get_inner_mut().inner = Some(inner); }
        }
    }

    let loopers = {
        let mut queue = SOUND_QUEUE.lock().unwrap();
        std::mem::take(&mut queue.to_play_looping)
    };

    for sound in loopers {
        play_sound_looping_impl(sound);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn load_sound(gc: &mut GcContext, path: Gp<PsStrBuf>, _closure: *mut c_void) -> Gp<PqSound> {
    let sound = PqSound {
        // Opaque, as we don't have any inner members.
        header: PsObject::from_type_id(PONI_TAG_OPAQUE),
        inner: None,
    };

    let result = gc.alloc(sound);

    // Add it to the queue.
    let mut queue = SOUND_QUEUE.lock().unwrap();
    queue.push(path, result.clone());

    result
}

#[unsafe(no_mangle)]
pub extern "C" fn play_sound_once(gc: &mut GcContext, sound: Gp<PqSound>, _closure: *mut c_void) {
    let inner = sound.get_inner();
    if let Some(inner) = &inner.inner {
        macroquad::audio::play_sound_once(inner);
    }
}

fn play_sound_looping_impl(sound: Gp<PqSound>) {
    let inner = sound.get_inner();
    if let Some(inner) = &inner.inner {
        macroquad::audio::play_sound(inner, PlaySoundParams {
            looped: true,
            volume: 1.0,
        })
    }
    else {
        let mut queue = SOUND_QUEUE.lock().unwrap();
        queue.push_looper(sound);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn play_sound_looping(gc: &mut GcContext, sound: Gp<PqSound>, _closure: *mut c_void) {
    play_sound_looping_impl(sound);   
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_stop_sound(gc: &mut GcContext, sound: Gp<PqSound>, _closure: *mut c_void) {
    let inner = sound.get_inner();
    if let Some(inner) = &inner.inner {
        macroquad::audio::stop_sound(inner);
    }
    else {
        // TODO: How to handle this...?
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_is_sound_loaded(gc: &mut GcContext, sound: Gp<PqSound>, _closure: *mut c_void) -> PsBool {
    let inner = sound.get_inner();
    return ps_bool(inner.inner.is_some());
}