use std::{os::raw::c_void, sync::Mutex};

use macroquad::texture::DrawTextureParams;
use poniescript_gc::{GcContext, Gp, HasPsHeader, HasPsType, PONI_TAG_OPAQUE, PsFloat, PsInt, PsObject};
use poniescript_rt::{PsStrBuf, Vec2, Vec4};

#[repr(C)]
pub struct PqFont {
    header: PsObject,
    pub inner: Option<macroquad::prelude::Font>,
}

unsafe impl HasPsHeader for PqFont {}

impl HasPsType for PqFont {
    const TYP: u64 = PONI_TAG_OPAQUE;
}

/// Handles loading the textures "later."
/// 
/// Will need to be visited as a GC root...
struct FontQueue {
    outstanding: Vec<(Gp<PqFont>, String)>,
}

impl FontQueue {
    const fn new() -> Self {
        Self {
            outstanding: Vec::new()
        }
    }

    fn push(&mut self, path: Gp<PsStrBuf>, result: Gp<PqFont>) {
        self.outstanding.push((result, path.get_inner().get_string().into_owned()));
    }
}

static FONT_QUEUE: Mutex<FontQueue> = Mutex::new(FontQueue::new()); 

pub async fn process_queue() {
    let queue = {
        let mut queue = FONT_QUEUE.lock().unwrap();
        std::mem::take(&mut queue.outstanding)
    };

    for (font, path) in queue {
        let inner = macroquad::prelude::load_ttf_font(&path).await;
        if let Ok(inner) = inner {
            // SAFETY: We don't support multithreading, so it is not possible
            // for another thread to interfere.
            //
            // If it was, we could add an additional level of indirection to make
            // it safe.
            unsafe { font.get_inner_mut().inner = Some(inner); }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn load_font(gc: &mut GcContext, path: Gp<PsStrBuf>, _closure: *mut c_void) -> Gp<PqFont> {
    let texture = PqFont {
        // Opaque, as we don't have any inner members.
        header: PsObject::from_type_id(PONI_TAG_OPAQUE),
        inner: None,
    };

    let result = gc.alloc(texture);

    // Add it to the queue.
    let mut queue = FONT_QUEUE.lock().unwrap();
    queue.push(path, result.clone());

    result
}