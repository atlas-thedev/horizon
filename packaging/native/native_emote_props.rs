//! Staged Native emote prop parser and sampler. Not yet wired into Horizon rendering.
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Texture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub uv_width: f32,
    pub uv_height: f32,
    pub emissive: bool,
    /// Populated once on the asset worker, after fully decoding and checking the PNG.
    pub rgba: Arc<[u8]>,
}
#[derive(Clone, Debug)]
pub struct Face {
    pub direction: String,
    pub uv: [f32; 4],
    pub texture: usize,
    pub rotation: i32,
}
#[derive(Clone, Debug)]
pub struct Cube {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub origin: [f32; 3],
    pub rotation: [f32; 3],
    pub inflate: f32,
    pub faces: Vec<Face>,
}
#[derive(Clone, Debug)]
struct Key { time: f64, value: [f32; 3], interpolation: String }
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub origin: [f32; 3],
    pub rotation: [f32; 3],
    pub cubes: Vec<Cube>,
    channels: [Vec<Key>; 3],
}
#[derive(Clone, Debug)]
pub struct PropClip { pub textures: Vec<Texture>, pub nodes: Vec<Node>, pub duration_ms: u32, pub looping: bool }

fn num(v: &Value, limit: f64) -> Option<f64> {
    let n = v.as_f64()?;
    (n.is_finite() && n.abs() <= limit).then_some(n)
}
fn vecn<const N: usize>(v: &Value) -> Option<[f32; N]> {
    let a = v.as_array()?;
    if a.len() != N { return None; }
    let mut out=[0.0; N];
    for (i,v) in a.iter().enumerate() { out[i]=num(v,10000.0)? as f32; }
    Some(out)
}
fn png(encoded: &str) -> Option<Vec<u8>> {
    let data=encoded.strip_prefix("data:image/png;base64,")?.as_bytes();
    if data.len() > 1_398_104 || data.len()%4 != 0 { return None; }
    let mut out=Vec::with_capacity(data.len()/4*3);
    let decode=|c: u8| match c {
        b'A'..=b'Z'=>Some(c-b'A'), b'a'..=b'z'=>Some(c-b'a'+26),
        b'0'..=b'9'=>Some(c-b'0'+52), b'+'=>Some(62), b'/'=>Some(63), _=>None,
    };
    for (i,c) in data.chunks_exact(4).enumerate() {
        let last=i+1==data.len()/4;
        let a=decode(c[0])? as u32; let b=decode(c[1])? as u32;
        let pad2=c[2]==b'='; let pad3=c[3]==b'=';
        if (!last && (pad2||pad3)) || (pad2&&!pad3) { return None; }
        let d=if pad2 {0} else {decode(c[2])? as u32};
        let e=if pad3 {0} else {decode(c[3])? as u32};
        let value=(a<<18)|(b<<12)|(d<<6)|e;
        out.push((value>>16) as u8);
        if !pad2 {out.push((value>>8) as u8);}
        if !pad3 {out.push(value as u8);}
    }
    (out.len() >= 24 && out.len() <= 1024*1024 && out.starts_with(b"\x89PNG\r\n\x1a\n") && &out[12..16] == b"IHDR").then_some(out)
}
impl PropClip {
    pub fn parse(v: &Value, duration_ms: u32, looping: bool) -> Option<Self> {
        if v["version"].as_u64()? != 1 || !(1..=30000).contains(&duration_ms) ||
            (num(&v["duration"],30.0)?*1000.0).round() != duration_ms as f64 || v["loop"].as_bool()? != looping { return None; }
        let ts=v["textures"].as_array()?;
        if ts.len()>16 { return None; }
        let mut textures=Vec::new(); let mut pixels=0u64;
        for t in ts {
            let png=png(t["source"].as_str()?)?;
            let width=u32::from_be_bytes(png[16..20].try_into().ok()?);
            let height=u32::from_be_bytes(png[20..24].try_into().ok()?);
            if !(1..=2048).contains(&width) || !(1..=2048).contains(&height) || t["width"].as_u64()?!=width as u64 || t["height"].as_u64()?!=height as u64 {return None;}
            pixels+=width as u64 * height as u64; if pixels>4*1024*1024 {return None;}
            let u=num(&t["uvWidth"],2048.0)? as f32; let v=num(&t["uvHeight"],2048.0)? as f32;
            if u<=0.0 || v<=0.0 {return None;}
            textures.push(Texture {png,width,height,uv_width:u,uv_height:v,emissive:t["emissive"].as_bool()?,rgba:Arc::from([])});
        }
        let raw_nodes=v["nodes"].as_array()?; if raw_nodes.len()>128 {return None;}
        let mut nodes=Vec::new(); let mut cube_count=0; let mut key_count=0;
        for n in raw_nodes {
            let name=n["name"].as_str()?.to_owned(); if name.is_empty() || name.len()>512 {return None;}
            let p=n["parent"].as_i64()?; if p < -1 || p >= nodes.len() as i64 {return None;}
            let raw_cubes=n["cubes"].as_array()?; cube_count+=raw_cubes.len(); if cube_count>512 {return None;}
            let mut cubes=Vec::new();
            for c in raw_cubes {
                let mut faces=Vec::new();
                for (name,f) in c["faces"].as_object()? {
                    if !["north","south","east","west","up","down"].contains(&name.as_str()) {return None;}
                    let texture=usize::try_from(f["texture"].as_u64()?).ok()?; if texture>=textures.len() {return None;}
                    let rotation=i32::try_from(f["rotation"].as_i64()?).ok()?; if !(-360..=360).contains(&rotation) || rotation%90!=0 {return None;}
                    faces.push(Face {direction:name.clone(),uv:vecn(&f["uv"])?,texture,rotation});
                }
                cubes.push(Cube {from:vecn(&c["from"])?,to:vecn(&c["to"])?,origin:vecn(&c["origin"])?,rotation:vecn(&c["rotation"])?,inflate:num(&c["inflate"],512.0)? as f32,faces});
            }
            let mut channels:[Vec<Key>;3]=std::array::from_fn(|_| Vec::new());
            let raw_keys=n["keyframes"].as_array()?; key_count+=raw_keys.len(); if key_count>50000 {return None;}
            for k in raw_keys {
                let index=match k["channel"].as_str()? {"position"=>0,"rotation"=>1,"scale"=>2,_=>return None};
                let interpolation=k["interpolation"].as_str()?;
                if !["linear","step","catmullrom"].contains(&interpolation) {return None;}
                let time=num(&k["time"],30.0)?; if time<0.0 || time>v["duration"].as_f64()? {return None;}
                channels[index].push(Key{time,value:vecn(&k["value"])?,interpolation:interpolation.to_owned()});
            }
            for keys in &mut channels { keys.sort_by(|a,b| a.time.total_cmp(&b.time)); }
            nodes.push(Node{name,parent:if p<0 {None} else {Some(p as usize)},origin:vecn(&n["origin"])?,rotation:vecn(&n["rotation"])?,cubes,channels});
        }
        Some(Self {textures,nodes,duration_ms,looping})
    }
    /// Decode off the render thread. Do not accept mismatched dimensions or partial buffers.
    pub fn decode_textures(&mut self, mut decode: impl FnMut(&[u8]) -> Option<(u32,u32,Vec<u8>)>) -> Option<()> {
        for texture in &mut self.textures {
            let (width,height,rgba)=decode(&texture.png)?;
            if width!=texture.width || height!=texture.height || rgba.len()!=(width as usize)*(height as usize)*4 {return None;}
            texture.rgba=Arc::from(rgba);
        }
        Some(())
    }
    pub fn decoded_bytes(&self) -> usize { self.textures.iter().map(|t|t.rgba.len()).sum() }
    /// If the chosen body thumbnail hides every prop, find a sampled frame where it is visible.
    pub fn preview_time(&self, preferred:u64) -> u64 {
        if self.quads(preferred).is_some_and(|q|!q.is_empty()) {return preferred;}
        let mut best=(0.0f64,preferred);
        for i in 0..=28 {
            let time=(self.duration_ms as u64)*i/28;
            let Some(quads)=self.quads(time) else {continue};
            let area:f64=quads.iter().map(|q| {
                let a=std::array::from_fn::<_,3,_>(|j|(q.xyz[1][j]-q.xyz[0][j]) as f64);
                let b=std::array::from_fn::<_,3,_>(|j|(q.xyz[3][j]-q.xyz[0][j]) as f64);
                let c=[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
                c.iter().map(|n|n*n).sum::<f64>().sqrt()
            }).sum();
            if area.is_finite() && area>best.0 {best=(area,time);}
        }
        best.1
    }
    /// Column-major matrices in Blockbench model space, one per node.
    pub fn sample(&self, elapsed_ms: u64) -> Option<Vec<[f32;16]>> {
        let t=if self.looping { elapsed_ms%self.duration_ms as u64 } else { elapsed_ms.min(self.duration_ms as u64) } as f64 /1000.0;
        let mut matrices=Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let p=channel(&node.channels[0],t,0.0); let r=channel(&node.channels[1],t,0.0); let s=channel(&node.channels[2],t,1.0);
            let parent=node.parent.map(|i| &self.nodes[i]);
            let pos=std::array::from_fn(|j|node.origin[j]-parent.map_or(0.0,|n|n.origin[j])+p[j]);
            let rot=std::array::from_fn(|j|node.rotation[j]+r[j]);
            let local=local(pos,rot,s);
            let matrix=node.parent.map_or(local,|i|mul(matrices[i],local));
            if matrix.iter().any(|v|!v.is_finite()||v.abs()>1_000_000.0) {return None;}
            matrices.push(matrix);
        }
        Some(matrices)
    }
}
fn channel(keys:&[Key], t:f64, default:f32)->[f32;3] {
    if keys.is_empty(){return [default;3];}
    let i=keys.partition_point(|k|k.time<=t).saturating_sub(1);
    let a=&keys[i];let b=&keys[(i+1).min(keys.len()-1)];
    if std::ptr::eq(a,b)||a.interpolation=="step"{return a.value;}
    let f=((t-a.time)/(b.time-a.time)).clamp(0.0,1.0);
    std::array::from_fn(|j| {
        let v=a.value[j] as f64; let b=b.value[j] as f64;
        if a.interpolation=="catmullrom" {
            let p=keys[i.saturating_sub(1)].value[j] as f64;let n=keys[(i+2).min(keys.len()-1)].value[j] as f64;
            (0.5*(2.0*v+(-p+b)*f+(2.0*p-5.0*v+4.0*b-n)*f*f+(-p+3.0*v-3.0*b+n)*f*f*f)) as f32
        } else {(v+(b-v)*f) as f32}
    })
}
fn mul(a:[f32;16],b:[f32;16])->[f32;16] {std::array::from_fn(|i|(0..4).map(|k|a[i%4+k*4]*b[i/4*4+k]).sum())}
fn local(p:[f32;3],r:[f32;3],s:[f32;3])->[f32;16] {
    let (sx,cx)=r[0].to_radians().sin_cos();let (sy,cy)=r[1].to_radians().sin_cos();let (sz,cz)=r[2].to_radians().sin_cos();
    [cy*cz*s[0],cy*sz*s[0],-sy*s[0],0.0,(sx*sy*cz-cx*sz)*s[1],(sx*sy*sz+cx*cz)*s[1],sx*cy*s[1],0.0,
     (cx*sy*cz+sx*sz)*s[2],(cx*sy*sz-sx*cz)*s[2],cx*cy*s[2],0.0,p[0],p[1],p[2],1.0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture(looping: bool) -> Value {
        json!({"version":1,"duration":1,"loop":looping,"textures":[],"nodes":[
            {"name":"root","parent":-1,"origin":[0,0,0],"rotation":[0,0,0],"cubes":[],
             "keyframes":[{"channel":"position","time":0,"interpolation":"linear","value":[1,2,3]}]},
            {"name":"logo","parent":0,"origin":[0,0,0],"rotation":[0,0,90],"cubes":[],
             "keyframes":[{"channel":"scale","time":0,"interpolation":"linear","value":[0,0,0]},
                          {"channel":"scale","time":1,"interpolation":"linear","value":[1,1,1]}]}
        ]})
    }
    #[test] fn hierarchy_and_loop() {
        let clip=PropClip::parse(&fixture(true),1000,true).unwrap();
        let m=clip.sample(500).unwrap()[1];
        assert!(m[0].abs()<0.0001 && (m[1]-0.5).abs()<0.0001);
        assert_eq!(&m[12..15],&[1.0,2.0,3.0]);
        assert_eq!(clip.sample(1000).unwrap()[1][1],0.0);
        assert!((clip.sample(1500).unwrap()[1][1]-0.5).abs()<0.0001);
    }
    #[test] fn clamp_and_bad_inputs() {
        let clip=PropClip::parse(&fixture(false),1000,false).unwrap();
        assert!((clip.sample(5000).unwrap()[1][1]-1.0).abs()<0.0001);
        let mut v=fixture(true); v["nodes"][1]["parent"]=json!(1);
        assert!(PropClip::parse(&v,1000,true).is_none());
        let mut v=fixture(true); v["nodes"][1]["keyframes"][0]["value"][0]=json!("query.time");
        assert!(PropClip::parse(&v,1000,true).is_none());
        assert!(PropClip::parse(&fixture(true),999,true).is_none());
    }
    #[test] fn emits_rotated_uv_and_emissive_geometry() {
        let png="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
        let mut v=fixture(false);
        v["textures"]=json!([{"source":png,"width":1,"height":1,"uvWidth":16,"uvHeight":16,"emissive":true}]);
        v["nodes"][1]["rotation"]=json!([0,0,0]);
        v["nodes"][1]["keyframes"]=json!([]);
        v["nodes"][1]["cubes"]=json!([{"from":[0,0,0],"to":[16,16,16],"origin":[0,0,0],"rotation":[0,0,0],"inflate":0,
            "faces":{"north":{"uv":[0,0,16,16],"texture":0,"rotation":90}}}]);
        let clip=PropClip::parse(&v,1000,false).unwrap(); let q=clip.quads(0).unwrap();
        assert_eq!(q.len(),1); assert!(q[0].emissive);
        assert_eq!(q[0].uv[0],[1.,1.]);
        assert!((q[0].normal[2]+1.).abs()<0.0001);
        assert!((q[0].xyz[0][0]+17.0/16.0).abs()<0.0001);
    }
    #[test] fn preview_avoids_zero_scale_frame() {
        let mut v=fixture(false);
        let png="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
        v["textures"]=json!([{"source":png,"width":1,"height":1,"uvWidth":16,"uvHeight":16,"emissive":true}]);
        v["nodes"][1]["rotation"]=json!([0,0,0]);
        v["nodes"][1]["keyframes"]=json!([
            {"channel":"scale","time":0,"interpolation":"linear","value":[1,1,1]},
            {"channel":"scale","time":0.5,"interpolation":"linear","value":[0,0,0]},
            {"channel":"scale","time":1,"interpolation":"linear","value":[1,1,1]}]);
        v["nodes"][1]["cubes"]=json!([{"from":[0,0,0],"to":[16,16,16],"origin":[0,0,0],"rotation":[0,0,0],"inflate":0,
            "faces":{"north":{"uv":[0,0,16,16],"texture":0,"rotation":0}}}]);
        let clip=PropClip::parse(&v,1000,false).unwrap();
        assert!(clip.quads(500).unwrap().is_empty());
        let t=clip.preview_time(500);assert_ne!(t,500);assert!(!clip.quads(t).unwrap().is_empty());
    }
    #[test] fn decoder_rejects_mismatches_and_keeps_exact_bytes() {
        let mut v=fixture(false);
        let png="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
        v["textures"]=json!([{"source":png,"width":1,"height":1,"uvWidth":1,"uvHeight":1,"emissive":false}]);
        let mut c=PropClip::parse(&v,1000,false).unwrap();
        assert!(c.decode_textures(|_|Some((2,1,vec![0;8]))).is_none());
        assert!(c.decode_textures(|_|Some((1,1,vec![0;3]))).is_none());
        assert!(c.decode_textures(|_|Some((1,1,vec![1,2,3,4]))).is_some());
        assert_eq!(&*c.textures[0].rgba,&[1,2,3,4]);
        assert_eq!(c.decoded_bytes(),4);
    }
    #[test] fn overflowing_uv_is_not_emitted() {
        let mut v=fixture(false);
        let png="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
        v["textures"]=json!([{"source":png,"width":1,"height":1,"uvWidth":1.4e-45,"uvHeight":16,"emissive":true}]);
        v["nodes"][1]["keyframes"]=json!([]);
        v["nodes"][1]["cubes"]=json!([{"from":[0,0,0],"to":[16,16,16],"origin":[0,0,0],"rotation":[0,0,0],"inflate":0,
            "faces":{"north":{"uv":[0,0,16,16],"texture":0,"rotation":0}}}]);
        let clip=PropClip::parse(&v,1000,false).unwrap();
        assert!(clip.quads(0).unwrap().is_empty());
    }
    #[test] fn texture_rejects_bad_data() {
        assert!(png("data:image/png;base64,AAAA").is_none());
        assert!(png("data:image/png;base64,----").is_none());
        let good="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
        let bytes=png(good).unwrap();assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()),1);
    }
}

/// One CPU-generated quad. Coordinates are player-model blocks, UVs are texture-normalized.
#[derive(Clone, Debug)]
pub struct Quad { pub texture:usize, pub emissive:bool, pub xyz:[[f32;3];4], pub uv:[[f32;2];4], pub normal:[f32;3] }
impl PropClip {
    pub fn quads(&self, elapsed_ms:u64) -> Option<Vec<Quad>> {
        let matrices=self.sample(elapsed_ms)?; let mut out=Vec::new();
        for (i,node) in self.nodes.iter().enumerate() {
            for cube in &node.cubes {
                for face in &cube.faces {
                    let corners=corners(&face.direction)?; let texture=self.textures.get(face.texture)?;
                    let mut xyz=[[0.0;3];4]; let mut uv=[[0.0;2];4];
                    for (v,f) in corners.iter().enumerate() {
                        let mut p=std::array::from_fn(|j|cube.from[j]-cube.inflate+f[j]*(cube.to[j]-cube.from[j]+2.0*cube.inflate)-cube.origin[j]);
                        rotate(&mut p,cube.rotation);
                        for j in 0..3 {p[j]+=cube.origin[j]-node.origin[j];}
                        let m=matrices[i];
                        let x=m[0]*p[0]+m[4]*p[1]+m[8]*p[2]+m[12];
                        let y=m[1]*p[0]+m[5]*p[1]+m[9]*p[2]+m[13];
                        let z=m[2]*p[0]+m[6]*p[1]+m[10]*p[2]+m[14];
                        xyz[v]=[-x/16.0,(24.0-y)/16.0,z/16.0];
                        let [s,t]=face_st(&face.direction,*f)?;
                        let [s,t]=match face.rotation.rem_euclid(360) {90=>[t,1.0-s],180=>[1.0-s,1.0-t],270=>[1.0-t,s],_=>[s,t]};
                        uv[v]=[(face.uv[0]+s*(face.uv[2]-face.uv[0]))/texture.uv_width,
                            (face.uv[1]+t*(face.uv[3]-face.uv[1]))/texture.uv_height];
                    }
                    // Very small positive UV dimensions can overflow even with finite inputs.
                    if xyz.iter().flatten().chain(uv.iter().flatten()).any(|v| !v.is_finite()) {continue;}
                    let a=std::array::from_fn::<_,3,_>(|j|xyz[1][j]-xyz[0][j]);
                    let b=std::array::from_fn::<_,3,_>(|j|xyz[2][j]-xyz[0][j]);
                    let normal=[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
                    let length=normal.iter().map(|v|v*v).sum::<f32>().sqrt();
                    if !length.is_finite() || length<1e-12 {continue;}
                    out.push(Quad{texture:face.texture,emissive:texture.emissive,xyz,uv,normal:normal.map(|v|v/length)});
                }
            }
        }
        Some(out)
    }
}
fn rotate(p:&mut [f32;3],r:[f32;3]) {
    let (sx,cx)=r[0].to_radians().sin_cos(); let (sy,cy)=r[1].to_radians().sin_cos(); let (sz,cz)=r[2].to_radians().sin_cos();
    let y=cx*p[1]-sx*p[2];let z=sx*p[1]+cx*p[2];let x=cy*p[0]+sy*z;let z=-sy*p[0]+cy*z;
    *p=[cz*x-sz*y,sz*x+cz*y,z];
}
fn face_st(face:&str,[x,y,z]:[f32;3])->Option<[f32;2]> {
    Some(match face {"west"=>[z,1.0-y],"east"=>[1.0-z,1.0-y],"north"=>[1.0-x,1.0-y],"south"=>[x,1.0-y],"up"=>[x,z],"down"=>[x,1.0-z],_=>return None})
}
fn corners(face:&str)->Option<[[f32;3];4]> {
    Some(match face {
        "north"=>[[1.,0.,0.],[0.,0.,0.],[0.,1.,0.],[1.,1.,0.]],
        "south"=>[[0.,0.,1.],[1.,0.,1.],[1.,1.,1.],[0.,1.,1.]],
        "west"=>[[0.,0.,0.],[0.,0.,1.],[0.,1.,1.],[0.,1.,0.]],
        "east"=>[[1.,0.,1.],[1.,0.,0.],[1.,1.,0.],[1.,1.,1.]],
        "up"=>[[1.,1.,0.],[0.,1.,0.],[0.,1.,1.],[1.,1.,1.]],
        "down"=>[[1.,0.,1.],[0.,0.,1.],[0.,0.,0.],[1.,0.,0.]],_=>return None,
    })
}
