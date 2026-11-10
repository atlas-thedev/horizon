//! Native animated props on the entity atlas. Asset parsing and PNG decoding run off-thread.
use super::Game;
use horizon_client::mesher::Vertex;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

struct Upload { key: String, width: u32, height: u32, rgba: Arc<[u8]> }
#[derive(Default)]
struct Uploads { queue: VecDeque<Upload>, keys: HashSet<String>, bytes: usize }
fn uploads() -> std::sync::MutexGuard<'static, Uploads> {
    static UPLOADS: OnceLock<Mutex<Uploads>> = OnceLock::new();
    UPLOADS.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn enqueue(key: String, texture: &horizon_client::native_emote_props::Texture) {
    const MAX_QUEUE: usize = 16;
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    let mut q=uploads();
    if q.keys.contains(&key) || q.queue.len() >= MAX_QUEUE || q.bytes + texture.rgba.len() > MAX_BYTES || texture.rgba.is_empty() {return;}
    q.bytes+=texture.rgba.len(); q.keys.insert(key.clone());
    q.queue.push_back(Upload{key,width:texture.width,height:texture.height,rgba:texture.rgba.clone()});
}

impl Game<'_> {
    /// Called once before entity drawing. Atlas writes never happen inside the render walk.
    pub(super) fn native_emote_prop_uploads(&mut self) {
        for _ in 0..2 {
            let job={
                let mut q=uploads();
                let job=q.queue.pop_front();
                if let Some(job)=&job { q.bytes-=job.rgba.len(); q.keys.remove(&job.key); }
                job
            };
            let Some(job)=job else {break};
            if !self.gui.atlas.sprites.contains_key(&job.key) {
                self.store_native_emote_texture(&job.key,job.width,job.height,&job.rgba);
            }
        }
    }

    /// The entity's own model transform and the body's playback clock are shared with the prop.
    pub(super) fn native_emote_props_geometry(&self, id:u32, base:glam::Mat4, light:u32, out:&mut Vec<Vertex>) {
        let Some((hash, clip, elapsed))=horizon_client::native_emotes::prop_frame(id) else {return};
        let Some(quads)=clip.quads(elapsed) else {return};
        if quads.is_empty() {return;}
        let mut sprites=Vec::with_capacity(clip.textures.len());
        for (index, texture) in clip.textures.iter().enumerate() {
            let key=format!("native/emote/{hash}/{index}");
            if let Some(rect)=self.gui.atlas.sprites.get(&key) {
                super::native_cosmetics::drawn(&key);
                sprites.push(Some([rect.u0,rect.v0,rect.u1,rect.v1]));
            } else {enqueue(key,texture); sprites.push(None);}
        }
        for q in quads {
            let Some(Some(uv))=sprites.get(q.texture) else {continue};
            let pos=q.xyz.map(|p|base.transform_point3(glam::Vec3::from_array(p)));
            if pos.iter().any(|v|!v.is_finite()) {continue;}
            let normal=(pos[1]-pos[0]).cross(pos[2]-pos[0]).normalize_or_zero();
            if normal.length_squared()<1e-12 {continue;}
            let encode=|n:f32|(n*127.0).round().clamp(-127.0,127.0) as i8;
            let normal=if q.emissive {[0;4]} else {[encode(normal.x),encode(normal.y),encode(normal.z),horizon_client::mesher::SHADED_CULL_W]};
            let layer=horizon_client::gfx::OIT_ENTITY | if q.emissive {(horizon_client::mesher::BRIGHT as u32)<<16} else {0};
            for i in 0..4 {
                out.push(Vertex {pos:pos[i].to_array(),uv:[uv[0]+(uv[2]-uv[0])*q.uv[i][0].clamp(0.0,1.0),uv[1]+(uv[3]-uv[1])*q.uv[i][1].clamp(0.0,1.0)],
                    layer,color:0xffff_ffff,light:if q.emissive {240|240<<8} else {light},normal,glint_uv:[0.0;2]});
            }
        }
    }
}
