use std::{os::raw::c_void, sync::Mutex};

use macroquad::audio::PlaySoundParams;
use poniescript_gc::{GcContext, Gp, GpMaybe, HasPsHeader, HasPsType, PONI_TAG_OPAQUE, PsBool, PsFloat, PsInt, PsObject, ps_bool};
use poniescript_rt::{PsStrBuf, Vec2, Vec4};

struct PqString {
    header: PsObject,
    inner: Option<String>,
}

unsafe impl HasPsHeader for PqString {}

impl HasPsType for PqString {
    const TYP: u64 = PONI_TAG_OPAQUE;
}

struct StringQueue {
    outstanding: Vec<(Gp<PqString>, String)>
}

impl StringQueue {
    const fn new() -> Self {
        Self { outstanding: Vec::new() }
    }

    fn push(&mut self, path: Gp<PsStrBuf>, result: Gp<PqString>) {
        self.outstanding.push((result, path.get_inner().get_string().into_owned()));
    }
}

static STRING_QUEUE: Mutex<StringQueue> = Mutex::new(StringQueue::new());

pub async fn process_queue() {
    let queue = {
        let mut queue = STRING_QUEUE.lock().unwrap();
        std::mem::take(&mut queue.outstanding)
    };

    for (string, path) in queue {
        let inner = macroquad::file::load_string(&path).await;
        if let Ok(inner) = inner {
            // SAFETY: We don't support multithreading, so it is not possible
            // for another thread to interfere.
            //
            // If it was, we could add an additional level of indirection to make
            // it safe.
            unsafe { string.get_inner_mut().inner = Some(inner); }
        }
    }
}

#[unsafe(no_mangle)]
pub fn load_string(gc: &mut GcContext, path: Gp<PsStrBuf>, _closure: *mut c_void) -> Gp<PqString> {
    let string = PqString {
        header: PsObject::from_type_id(PONI_TAG_OPAQUE),
        inner: None,
    };

    let result = gc.alloc(string);

    let mut queue = STRING_QUEUE.lock().unwrap();
    queue.push(path, result.clone());

    result
}

#[unsafe(no_mangle)]
pub fn get_loaded_string(gc: &mut GcContext, string: Gp<PqString>) -> GpMaybe<PsStrBuf> {
    let inner = string.get_inner();
    if let Some(inner) = &inner.inner {
        let buf = PsStrBuf::from_string(gc, &inner);
        return GpMaybe::new(Some(buf));
    }

    return GpMaybe::new(None);
}