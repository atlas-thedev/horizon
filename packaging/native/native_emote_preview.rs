//! CPU texture sampling for Native emote wheel props. Images share a bounded UI cache.
use super::boxes::Boxes;
use horizon_client::native_emote_props::{PropClip, Quad};
use horizon_client::native_ui::Image;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
#[derive(Default)]
struct Cache {images:HashMap<String,Arc<Image>>,order:VecDeque<String>,pixels:usize}
fn cache()->std::sync::MutexGuard<'static,Cache> {
    static CACHE:OnceLock<Mutex<Cache>>=OnceLock::new();
    CACHE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn images(hash:&str,clip:&PropClip)->Vec<Option<Arc<Image>>> {
    let mut cache=cache(); let mut result=Vec::with_capacity(clip.textures.len());let mut decoded=false;
    for (index,t) in clip.textures.iter().enumerate() {
        let key=format!("{hash}/{index}");
        if let Some(image)=cache.images.get(&key).cloned() {
            cache.order.retain(|k|k!=&key);cache.order.push_back(key);result.push(Some(image));continue;
        }
        if decoded || t.rgba.len()!=t.width as usize*t.height as usize*4 {result.push(None);continue;}
        decoded=true;let pixels=t.width as usize*t.height as usize;
        while cache.images.len()>=16 || cache.pixels+pixels>8*1024*1024 {
            let Some(old)=cache.order.pop_front() else {break};
            if let Some(image)=cache.images.remove(&old) {cache.pixels-=image.width as usize*image.height as usize;}
        }
        if cache.pixels+pixels>8*1024*1024 {result.push(None);continue;}
        let argb=t.rgba.chunks_exact(4).map(|p|u32::from(p[3])<<24|u32::from(p[0])<<16|u32::from(p[1])<<8|u32::from(p[2])).collect();
        let image=Image::new(t.width,t.height,argb);
        cache.pixels+=pixels;cache.order.push_back(key.clone());cache.images.insert(key,image.clone());result.push(Some(image));
    }
    result
}
pub fn draw(boxes:&mut Boxes,hash:&str,clip:&PropClip,quads:&[Quad]) {
    let images=images(hash,clip);
    for q in quads {if let Some(Some(image))=images.get(q.texture) {boxes.prop_quad(image,q);}}
}
