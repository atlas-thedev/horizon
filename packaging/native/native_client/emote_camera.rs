//! Temporary third-person view for local emotes, without overriding a player's F5 choice.
#[derive(Default)]
pub struct Camera {
    playing: bool,
    switched: bool,
}
impl Camera {
    pub fn update(&mut self, active: bool, perspective: i32) -> Option<i32> {
        let next = if active && !self.playing && perspective == 0 {
            self.switched = true;
            Some(1)
        } else if !active && self.switched {
            self.switched = false;
            (perspective == 1).then_some(0)
        } else {
            None
        };
        self.playing = active;
        next
    }
}
#[cfg(test)]
mod tests {
    use super::Camera;
    #[test]
    fn restores_first_person_after_playback_and_respects_f5() {
        let mut c = Camera::default();
        assert_eq!(c.update(false, 0), None);
        assert_eq!(c.update(true, 0), Some(1));
        assert_eq!(c.update(true, 1), None);
        assert_eq!(c.update(false, 1), Some(0));
        assert_eq!(c.update(true, 0), Some(1));
        assert_eq!(c.update(true, 2), None);
        assert_eq!(c.update(false, 2), None);
        assert_eq!(c.update(true, 2), None);
        assert_eq!(c.update(false, 2), None);
    }
}
