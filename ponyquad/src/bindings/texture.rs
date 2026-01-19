use std::{os::raw::c_void, sync::Mutex};

use poniescript_gc::{GcContext, Gp, HasPsHeader, HasPsType, PsObject};
use poniescript_rt::{PsStrBuf, Vec2, Vec4};

#[repr(C)]
pub struct Texture2D {
    header: PsObject,
    inner: macroquad::texture::Texture2D,
}

unsafe impl HasPsHeader for Texture2D {}

impl HasPsType for Texture2D {
    const TYP: u64 = 18;
}

/// Handles loading the textures "later."
/// 
/// Will need to be visited as a GC root...
struct TextureQueue {
    outstanding: Vec<(Gp<Texture2D>, String)>,
}

impl TextureQueue {
    const fn new() -> Self {
        Self {
            outstanding: Vec::new()
        }
    }

    fn push(&mut self, path: Gp<PsStrBuf>, result: Gp<Texture2D>) {
        self.outstanding.push((result, path.get_inner().get_string().into_owned()));
    }
}

static TEXTURE_QUEUE: Mutex<TextureQueue> = Mutex::new(TextureQueue::new()); 

pub async fn process_queue() {
    let queue = {
        let mut queue = TEXTURE_QUEUE.lock().unwrap();
        std::mem::take(&mut queue.outstanding)
    };

    for (tex, path) in queue {
        let inner = macroquad::prelude::load_texture(&path).await;
        if let Ok(inner) = inner {
            // SAFETY: We don't support multithreading, so it is not possible
            // for another thread to interfere.
            //
            // If it was, we could add an additional level of indirection to make
            // it safe.
            unsafe { tex.get_inner_mut().inner = inner; }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn load_texture(gc: &mut GcContext, path: Gp<PsStrBuf>, _closure: *mut c_void) -> Gp<Texture2D> {
    let texture = Texture2D {
        header: PsObject::from_type_id(0),
        inner: macroquad::prelude::Texture2D::empty()
    };

    let result = gc.alloc(texture);

    // Add it to the queue.
    let mut queue = TEXTURE_QUEUE.lock().unwrap();
    queue.push(path, result.clone());

    result
}

#[unsafe(no_mangle)]
pub extern "C" fn draw_texture(_gc: &mut GcContext, texture: Gp<Texture2D>, pos: Vec2, color: Vec4, _closure: *mut c_void) {
    let color = unsafe { std::mem::transmute(color) };
    macroquad::prelude::draw_texture(&texture.get_inner().inner, pos.x, pos.y, color);

    //let inner = texture.get_inner();
    //eprintln!("drew texture: {}x{} @ {} {}", inner.inner.width(), inner.inner.height(), pos.x, pos.y);
}