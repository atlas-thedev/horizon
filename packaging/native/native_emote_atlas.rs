// Native emote props share the bounded cosmetic atlas packer and its reload/eviction lifecycle.
impl Game<'_> {
    pub(super) fn store_native_emote_texture(&mut self, location: &str, w:u32, h:u32, rgba:&[u8]) {
        if w==0 || h==0 || w>AREA || h>AREA || rgba.len()!=(w as usize)*(h as usize)*4 {return;}
        let Some(area)=self.gui.atlas.sprites.get("native/cosmetics").copied() else {return};
        let placed={
            let mut p=packer();
            if p.revision!=self.gui.atlas.revision { *p=Packer{revision:self.gui.atlas.revision,..Packer::default()}; }
            p.place(location,location,w,h,Instant::now())
        };
        let Some(placed)=placed else {return};
        for old in placed.evicted {self.gui.atlas.sprites.remove(&old);}
        let (aw,ah)=(self.gui.atlas.width as f32,self.gui.atlas.height as f32);
        let (ax,ay)=((area.u0*aw).round() as u32,(area.v0*ah).round() as u32);
        let rect=placed.rect;
        if placed.upload {self.map_patches.push(horizon_client::font::Patch{x:ax+rect[0],y:ay+rect[1],width:w,height:h,rgba:rgba.to_vec()});}
        let mut sprite=area;
        sprite.u0=(ax+rect[0]) as f32/aw; sprite.v0=(ay+rect[1]) as f32/ah;
        sprite.u1=(ax+rect[0]+w) as f32/aw; sprite.v1=(ay+rect[1]+h) as f32/ah;
        self.gui.atlas.sprites.insert(location.to_owned(),sprite);
    }
}
