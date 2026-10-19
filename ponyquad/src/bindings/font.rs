use std::{os::raw::c_void, sync::Mutex};

use macroquad::texture::DrawTextureParams;
use poniescript_gc::{GcContext, Gp, HasPsHeader, HasPsType, PONI_TAG_OPAQUE, PsBool, PsFloat, PsInt, PsObject, poni_tag_opaque};
use poniescript_rt::{PsStrBuf, Vec2, Vec4};

#[repr(C)]
pub struct PqFont {
    header: PsObject,
    pub inner: Option<macroquad::prelude::Font>,
}

unsafe impl HasPsHeader for PqFont {}

impl HasPsType for PqFont {
    const TYP: u64 = poni_tag_opaque::<Self>();
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
        header: PsObject::from_type_id(PqFont::TYP),
        inner: None,
    };

    let result = gc.alloc(texture);

    // Add it to the queue.
    let mut queue = FONT_QUEUE.lock().unwrap();
    queue.push(path, result.clone());

    result
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_is_font_loaded(gc: &mut GcContext, font: Gp<PqFont>, _closure: *mut c_void) -> PsBool {
    let font = font.get_inner();
    return if font.inner.is_some() { 1 } else { 0 };
}

// TODO: it would be nice if this didn't require the font to already be loaded to work. Oh well..
#[unsafe(no_mangle)]
pub extern "C" fn pq_set_font_filter_mode(_gc: &mut GcContext, font: Gp<PqFont>, mode: PsInt, _closure: *mut c_void) {
    let font = unsafe { font.get_inner_mut() };
    if let Some(font) = &mut font.inner {
        let mode = match mode {
            0 => macroquad::texture::FilterMode::Linear,
            _ => macroquad::texture::FilterMode::Nearest,
        };
        font.set_filter(mode);
    }
}
