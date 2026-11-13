
// ── Native cosmetics on the player hooks' drawing (tools/native/native_cosmetics.py copies this file next to
// game/player_hooks.rs and includes it there) ──────────────────────────────────────────────────────────────────────
// Since 2026-11-13 Horizon's cosmetics draw through the player hooks (`PlayerDraw`), whose sprites are 64×64 skin
// slots. Native's items (game/native_cosmetics.rs) are drawn the same way from here, in the engine, where the
// wearer's armour and name are known, textured from the atlas's Native area at their own size.

thread_local! {
    /// Native items' meshes, by the item's address (each entry holds its item, so the address isn't reused).
    #[allow(clippy::type_complexity)]
    static NATIVE_MESHES: std::cell::RefCell<FxHashMap<usize, (Arc<horizon_cosmetics::Item>, Vec<(Bone, Arc<Mesh>)>)>> =
        Default::default();
    /// Native items drawing found without their texture in the atlas: put there next frame.
    static NATIVE_WANTED: std::cell::RefCell<Vec<Arc<horizon_cosmetics::Item>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The hooks' model of an item's part (as `cosmetics.rs` makes Horizon's).
fn native_mesh(p: &horizon_cosmetics::Part) -> Mesh {
    let quads = p.faces.iter().map(|f| horizon_hooks::players::Quad::new(f.normal, f.vertices));
    Mesh::new(p.name.clone(), p.pose, quads.collect(), p.children.iter().map(native_mesh).collect())
}

fn native_bone(b: horizon_cosmetics::Bone) -> Bone {
    use horizon_cosmetics::Bone as B;
    match b {
        B::Head => Bone::Head,
        B::Body => Bone::Body,
        B::LeftArm => Bone::LeftArm,
        B::RightArm => Bone::RightArm,
        B::LeftLeg => Bone::LeftLeg,
        B::RightLeg => Bone::RightLeg,
    }
}

/// `item`'s meshes, one per bone it rides, made once.
fn native_meshes(item: &Arc<horizon_cosmetics::Item>) -> Vec<(Bone, Arc<Mesh>)> {
    NATIVE_MESHES.with_borrow_mut(|all| {
        let entry = all.entry(Arc::as_ptr(item) as usize).or_insert_with(|| {
            let meshes = item.roots().map(|(b, root)| (native_bone(b), Arc::new(native_mesh(root))));
            (item.clone(), meshes.collect())
        });
        entry.1.clone()
    })
}

/// The atlas sprite a Native item is drawn from.
fn native_location(item: &horizon_cosmetics::Item) -> String {
    format!("native/item/{}", item.id)
}

impl DrawPlayer<'_, '_> {
    /// The Native cosmetics on the model being drawn (`own`: the window's own), after the crates' layers. Each
    /// item is posed and animated on its bone as Horizon's are; a glowing part is fully lit.
    fn native_cosmetics(&mut self, own: bool) {
        if self.pass == Pass::FirstPerson || horizon_client::mobs::trader::is_invisible(self.m) {
            return;
        }
        let items = self.game.native_cosmetics_worn(self.m, own);
        if items.is_empty() {
            return;
        }
        let ticks = self.ticks();
        let worn = gear(self.m);
        for item in &items {
            // A blinking part is off for part of its cycle (the Java mod skips it); gear covers its slot.
            if !super::native_cosmetics::shown(item, ticks) || super::cosmetics::covered(worn, item.slot) {
                continue;
            }
            let location = native_location(item);
            let Some(r) = self.game.gui.atlas.sprites.get(&location) else {
                NATIVE_WANTED.with_borrow_mut(|w| {
                    if !w.iter().any(|i| Arc::ptr_eq(i, item)) {
                        w.push(item.clone());
                    }
                });
                continue;
            };
            let uv = [r.u0, r.v0, r.u1, r.v1];
            super::native_cosmetics::drawn(&location);
            let glow = item.id.ends_with(super::native_cosmetics::GLOW);
            let look = Look::new(uv).lit(if glow { Light::Full } else { Light::Model });
            for (bone, mesh) in native_meshes(item) {
                let Some(matrix) = self.bone(bone) else {
                    continue;
                };
                self.draw(&mesh, matrix, look, &mut |p| {
                    item.animate(ticks, |name, pose| *p.pose(name) = pose);
                });
            }
        }
    }
}

impl Game<'_> {
    /// Once a frame, after the crates' frame hook: the Native textures drawing missed go into the atlas, and the
    /// meshes of items no one wears any more are forgotten.
    pub(super) fn native_cosmetics_frame(&mut self) {
        for item in NATIVE_WANTED.with_borrow_mut(std::mem::take) {
            self.store_native_cosmetic(&native_location(&item), &item);
        }
        NATIVE_MESHES.with_borrow_mut(|all| all.retain(|_, (item, _)| Arc::strong_count(item) > 1));
    }
}
