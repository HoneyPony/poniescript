use std::{os::raw::c_void, sync::Mutex};

use macroquad::{audio::PlaySoundParams, math::Rect};
use macroquad_tiled::{Map, load_map};
use poniescript_gc::{GcContext, Gp, GpMaybe, HasPsHeader, HasPsType, PONI_TAG_OPAQUE, PsBool, PsFloat, PsInt, PsObject, poni_tag_opaque, ps_bool};
use poniescript_rt::{PsArray, PsStr, PsStrBuf, Vec2, Vec2i, Vec4};

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
pub extern "C" fn load_string(gc: &mut GcContext, path: Gp<PsStrBuf>, _closure: *mut c_void) -> Gp<PqString> {
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
pub extern "C" fn get_loaded_string(gc: &mut GcContext, string: Gp<PqString>, _closure: *mut c_void) -> GpMaybe<PsStrBuf> {
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
pub extern "C" fn pq_load_map(gc: &mut GcContext, data: Gp<PsStrBuf>, tex_names: Gp<PsArray<Gp<PsStrBuf>>>, textures: Gp<PsArray<Gp<Texture2D>>>, _closure: *mut c_void) -> GpMaybe<PqTiledMap> {    
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

#[unsafe(no_mangle)]
pub extern "C" fn pq_draw_map(gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, dest: Vec4, _closure: *mut c_void) { 
    map.get_inner().inner.draw_tiles(&layer.get_inner().get_string(), Rect::new(dest.x, dest.y, dest.z, dest.w), None);
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_get_tile_map_size(gc: &mut GcContext, map: Gp<PqTiledMap>, _closure: *mut c_void) -> Vec2i {
    let mut size: Vec2i = Vec2i { x: 0, y: 0 };
    for layer in &map.get_inner().inner.layers {
        let layer = layer.1;
        size.x = size.x.max(layer.width as PsInt);
        size.y = size.y.max(layer.height as PsInt);
    }
    size
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_get_tile_map_tile(gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, coord: Vec2i, _closure: *mut c_void) -> PsInt {
    if coord.x < 0 || coord.y < 0 {
        return -1; // TODO: PonieScript option value types + some crazy way to bind them...
    }
    
    let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };

    tile.id as PsInt
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_set_tile_map_tile(gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, coord: Vec2i, new_id: PsInt, _closure: *mut c_void) -> PsBool {
    if coord.x < 0 || coord.y < 0 {
        return ps_bool(false);
    }

    // "SAFETY": ponyquad is single threaded only. Not really a good justification...
    unsafe {
        if let Some(layer) = map.get_inner_mut().inner.layers.get_mut(&layer.get_inner().get_string().into_owned()) {
            let idx = coord.y * layer.width as i64 + coord.x;
            if idx < 0 { return ps_bool(false); }
            let idx = idx as usize;
            if idx >= layer.data.len() { return ps_bool(false); }

            if let Some(tile) = &mut layer.data[idx] {
                tile.id = new_id as _;
                return ps_bool(true);
            }
        }
    }
    
    ps_bool(false)
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_remove_tile_map_tile(gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, coord: Vec2i, _closure: *mut c_void) -> PsBool {
    if coord.x < 0 || coord.y < 0 {
        return ps_bool(false);
    }

    // "SAFETY": ponyquad is single threaded only. Not really a good justification...
    unsafe {
        if let Some(layer) = map.get_inner_mut().inner.layers.get_mut(&layer.get_inner().get_string().into_owned()) {
            let idx = coord.y * layer.width as i64 + coord.x;
            if idx < 0 { return ps_bool(false); }
            let idx = idx as usize;
            if idx >= layer.data.len() { return ps_bool(false); }

            layer.data[idx] = None;
            return ps_bool(true);
        }
    }
    
    ps_bool(false)
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_tiledobj_get_count(_gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>) -> PsInt {
    let layer = layer.get_string();
    let layer = layer.as_ref();
    if let Some(layer) = map.get_inner().inner.layers.get(layer) {
        return layer.objects.len() as PsInt;
    }
    return 0;
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_tiledobj_get_property_float(_gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, index: PsInt, property: Gp<PsStrBuf>) -> PsFloat {
    let layer = layer.get_string();
    let layer = layer.as_ref();
    if let Some(layer) = map.get_inner().inner.layers.get(layer) {
        let property_name = property.get_string();

        return layer.objects[index as usize].properties.get(property_name.as_ref())
            .map(|s| s.parse::<f32>().ok())
            .flatten()
            .unwrap_or(0.0);
    }
    return 0.0;
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_tiledobj_get_property_int(_gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, index: PsInt, property: Gp<PsStrBuf>) -> PsInt {
    let layer = layer.get_string();
    let layer = layer.as_ref();
    if let Some(layer) = map.get_inner().inner.layers.get(layer) {
        let property_name = property.get_string();

        return layer.objects[index as usize].properties.get(property_name.as_ref())
            .map(|s| s.parse::<i64>().ok())
            .flatten()
            .unwrap_or(0);
    }
    return 0;
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}


#[unsafe(no_mangle)]
pub extern "C" fn pq_tiledobj_get_tile_id(_gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, index: PsInt) -> PsInt {
    let layer = layer.get_string();
    let layer = layer.as_ref();
    if let Some(layer) = map.get_inner().inner.layers.get(layer) {
        return layer.objects[index as usize].gid.map(|g| g as PsInt).unwrap_or(-1);
    }
    return -1;
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_tiledobj_get_position(_gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, index: PsInt) -> Vec2 {
    let layer = layer.get_string();
    let layer = layer.as_ref();
    if let Some(layer) = map.get_inner().inner.layers.get(layer) {
        return Vec2 {
            x: layer.objects[index as usize].world_x,
            y: layer.objects[index as usize].world_y,
        };
    }
    return Vec2 { x: 0.0, y: 0.0 };
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}

#[unsafe(no_mangle)]
pub extern "C" fn pq_tiledobj_get_text(gc: &mut GcContext, map: Gp<PqTiledMap>, layer: Gp<PsStrBuf>, index: PsInt) -> GpMaybe<PsStrBuf> {
    let layer = layer.get_string();
    let layer = layer.as_ref();
    if let Some(layer) = map.get_inner().inner.layers.get(layer) {
        let object = &layer.objects[index as usize];
        return object.text.as_ref().map(|m| GpMaybe::new(Some(PsStrBuf::from_string(gc, &m))))
            .unwrap_or_else(|| GpMaybe::none());
    }
    return GpMaybe::none();
    //let Some(tile) = map.get_inner().inner.get_tile(&layer.get_inner().get_string(), coord.x as u32, coord.y as u32) else { return -1; };
}