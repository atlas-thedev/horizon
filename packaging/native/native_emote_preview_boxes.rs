// Native emote prop texels: participate in the same sorted list as the avatar.
impl Boxes {
    pub fn prop_quad(&mut self, image:&Image, q:&horizon_client::native_emote_props::Quad) {
        const MAX_SAMPLES:usize=32768;
        if self.prop_samples>=MAX_SAMPLES {return;}
        let mut normal=self.normal(q.normal[0],q.normal[1],q.normal[2]);
        if normal[2].abs()<0.0001 || (normal[2]>=0.0 && !self.two_sided) {return;}
        if normal[2]>0.0 {normal=normal.map(|x|-x);}
        let bright=if q.emissive {1.0} else {0.58+0.42*(normal[0]* -0.35+normal[1]* -0.55+normal[2]* -0.76).max(0.0)};
        let points=q.xyz.map(|p|self.project(p[0]*16.0,p[1]*16.0,p[2]*16.0));
        let width=(((q.uv[1][0]-q.uv[0][0])*image.width as f32).hypot((q.uv[1][1]-q.uv[0][1])*image.height as f32).ceil() as usize).clamp(1,16);
        let height=(((q.uv[3][0]-q.uv[0][0])*image.width as f32).hypot((q.uv[3][1]-q.uv[0][1])*image.height as f32).ceil() as usize).clamp(1,16);
        let blend=|values:[f32;4],x:f32,y:f32|(1.0-y)*((1.0-x)*values[0]+x*values[1])+y*((1.0-x)*values[3]+x*values[2]);
        let xs=points.map(|p|p[0]);let ys=points.map(|p|p[1]);let zs=points.map(|p|p[2]);
        let us=q.uv.map(|p|p[0]);let vs=q.uv.map(|p|p[1]);
        for j in 0..height { for i in 0..width {
            if self.prop_samples>=MAX_SAMPLES {return;} self.prop_samples+=1;
            let (x,y)=((i as f32+0.5)/width as f32,(j as f32+0.5)/height as f32);
            let (u,v)=((blend(us,x,y)*image.width as f32).floor() as i64,(blend(vs,x,y)*image.height as f32).floor() as i64);
            if u<0 || v<0 || u>=image.width as i64 || v>=image.height as i64 {continue;}
            let pixel=image.argb[(v as usize)*image.width as usize+u as usize];
            if pixel>>24<8 {continue;}
            let (r,g,blue)=((((pixel>>16)&255) as f32*bright) as u32,(((pixel>>8)&255) as f32*bright) as u32,((pixel&255) as f32*bright) as u32);
            let point=|x:f32,y:f32|[blend(xs,x,y),blend(ys,x,y),blend(zs,x,y)];
            let (x0,x1)=(i as f32/width as f32,(i+1) as f32/width as f32);
            let (y0,y1)=(j as f32/height as f32,(j+1) as f32/height as f32);
            let [a,b,c,d]=[point(x0,y0),point(x1,y0),point(x1,y1),point(x0,y1)];
            self.quads.push(Quad{xy:[a[0],a[1],b[0],b[1],c[0],c[1],d[0],d[1]],z:(a[2]+b[2]+c[2]+d[2])/4.0,color:(pixel&0xff00_0000)|(r<<16)|(g<<8)|blue});
        }}
    }
}
