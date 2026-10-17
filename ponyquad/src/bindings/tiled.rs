use std::{os::raw::c_void, sync::Mutex};

use macroquad::audio::PlaySoundParams;
use macroquad_tiled::{Map, load_map};
use poniescript_gc::{GcContext, Gp, GpMaybe, HasPsHeader, HasPsType, PONI_TAG_OPAQUE, PsBool, PsFloat, PsInt, PsObject, poni_tag_opaque, ps_bool};
use poniescript_rt::{PsArray, PsStr, PsStrBuf, Vec2, Vec4};

use crate::bindings::texture::Texture2D;

#[repr(C)]
struct PqString {
    header: PsObject,
    inner: Option<String>,
}

unsafe impl HasPsHeader for PqString {}

impl HasPsType for PqString {
    const TYP: u64 = poni_tag_opaque::<Self>();
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
        header: PsObject::from_type_id(PqString::TYP),
        inner: None,
    };

    let result = gc.alloc(string);

    let mut queue = STRING_QUEUE.lock().unwrap();
    queue.push(path, result.clone());

    result
}

#[unsafe(no_mangle)]
pub fn get_loaded_string(gc: &mut GcContext, string: Gp<PqString>, _closure: *mut c_void) -> GpMaybe<PsStrBuf> {
    let inner = string.get_inner();
    if let Some(inner) = &inner.inner {
        let buf = PsStrBuf::from_string(gc, &inner);
        return GpMaybe::new(Some(buf));
    }

    return GpMaybe::new(None);
}

#[repr(C)]
struct PqTiledMap {
    header: PsObject,
    inner: Map,
}

unsafe impl HasPsHeader for PqTiledMap {}

impl HasPsType for PqTiledMap {
    const TYP: u64 = poni_tag_opaque::<Self>();
}

#[unsafe(no_mangle)]
pub fn pq_load_map(gc: &mut GcContext, data: Gp<PsStrBuf>, tex_names: Gp<PsArray<Gp<PsStrBuf>>>, textures: Gp<PsArray<Gp<Texture2D>>>, _closure: *mut c_void) -> GpMaybe<PqTiledMap> {    
    let mut tex_param = Vec::new();
    for i in 0..tex_names.get_inner().len() {
        if i >= textures.get_inner().len() {
            break;
        }

        let name = tex_names.get_inner().get(i);
        let tex = textures.get_inner().get(i);

        let name = name.get_inner().get_string();
        eprintln!("Got texture: {name} - {}", tex.get_inner().inner.size());
        tex_param.push((name, tex.get_inner().inner.clone()))
    }

    //return GpMaybe::new(None);

    let tex_param: Vec<_> = tex_param.iter().map(|p| (p.0.as_ref(), p.1.clone())).collect();
    let map = load_map(&data.get_inner().get_string(), &tex_param[..], &[]);

    match map {
        Ok(map) => {
            let ptr = gc.alloc(PqTiledMap { header: PsObject::from_type_id(PqTiledMap::TYP), inner: map });
            eprintln!("Allocated @ {:p} - {}", ptr.get_inner(), ptr.get_inner().header.type_id());
            GpMaybe::new(Some(ptr))
        }
        Err(err) => {
            // TODO: log error?
            GpMaybe::new(None)
        }
    }
}