const THRESHOLD: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragRequest(u64);

struct Press {
    request: DragRequest,
    origin: (f32, f32),
    crossed: bool,
}

/// A mouse gesture keeps its identity through async preparation and native completion.
#[derive(Default)]
pub struct DragGesture {
    generation: u64,
    press: Option<Press>,
    suppress_click: bool,
    active: bool,
}

impl DragGesture {
    pub fn begin(&mut self, x: f32, y: f32) -> DragRequest {
        self.generation = self.generation.wrapping_add(1);
        let request = DragRequest(self.generation);
        self.press = Some(Press {
            request,
            origin: (x, y),
            crossed: false,
        });
        self.suppress_click = false;
        request
    }

    /// Moving past the threshold consumes the click even if payload preparation fails.
    pub fn move_pointer(&mut self, x: f32, y: f32) {
        let Some(press) = &mut self.press else {
            return;
        };
        let dx = x - press.origin.0;
        let dy = y - press.origin.1;
        if dx * dx + dy * dy >= THRESHOLD * THRESHOLD {
            press.crossed = true;
            self.suppress_click = true;
        }
    }

    pub fn accepts(&self, request: DragRequest) -> bool {
        self.press
            .as_ref()
            .is_some_and(|press| press.request == request)
    }

    pub fn ready(&self, request: DragRequest) -> bool {
        !self.active
            && self
                .press
                .as_ref()
                .is_some_and(|press| press.request == request && press.crossed)
    }

    pub fn start(&mut self, request: DragRequest) -> bool {
        if !self.ready(request) {
            return false;
        }
        self.press = None;
        self.active = true;
        true
    }

    /// Release/close invalidates pending preparation without reviving the consumed click.
    pub fn release(&mut self) {
        self.press = None;
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn busy(&self) -> bool {
        self.active || self.press.as_ref().is_some_and(|press| press.crossed)
    }

    pub fn finish(&mut self) {
        self.active = false;
        self.press = None;
    }

    pub fn allows_click(&self) -> bool {
        !self.suppress_click && !self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_below_threshold_and_stale_preparation_do_not_start_a_drag() {
        let mut gesture = DragGesture::default();
        let old = gesture.begin(10.0, 10.0);
        gesture.move_pointer(13.0, 14.0);
        assert!(!gesture.ready(old));
        assert!(gesture.allows_click());
        gesture.release();
        assert!(!gesture.accepts(old));
        let current = gesture.begin(20.0, 20.0);
        gesture.move_pointer(27.0, 20.0);
        assert!(!gesture.ready(old));
        assert!(gesture.ready(current));
        assert!(!gesture.start(old));
        assert!(gesture.start(current));
    }

    #[test]
    fn success_cancel_and_missing_payload_never_replay_the_drag_as_click() {
        for native_starts in [false, true] {
            let mut gesture = DragGesture::default();
            let request = gesture.begin(0.0, 0.0);
            gesture.move_pointer(6.0, 0.0);
            assert!(!gesture.allows_click());
            if native_starts {
                assert!(gesture.start(request));
                assert!(gesture.active());
                gesture.finish();
            }
            gesture.release();
            assert!(!gesture.ready(request));
            assert!(!gesture.allows_click());
            gesture.begin(0.0, 0.0);
            assert!(gesture.allows_click());
        }
    }

    #[test]
    fn release_or_hide_before_preparation_completes_revokes_the_request() {
        let mut gesture = DragGesture::default();
        let request = gesture.begin(0.0, 0.0);
        gesture.move_pointer(50.0, 0.0);
        gesture.release();
        assert!(!gesture.accepts(request));
        assert!(!gesture.start(request));
        assert!(!gesture.allows_click());
    }
}
