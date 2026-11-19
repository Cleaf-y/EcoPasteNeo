//! A two-release gesture recognizer. It retains timing and pressed-key state only.

use std::{collections::HashSet, time::Duration};

#[derive(Debug, Clone, Copy)]
pub enum TapEvent {
    Modifier {
        key: u16,
        selected: bool,
        alone: bool,
        released: bool,
    },
    KeyDown(u16),
    KeyUp(u16),
    Cancel,
}

pub struct DoubleTap {
    interval: Duration,
    first: Option<Duration>,
    down: Option<(u16, Duration)>,
    neutral: bool,
    keys: HashSet<u16>,
}

impl DoubleTap {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            first: None,
            down: None,
            neutral: true,
            keys: HashSet::new(),
        }
    }

    /// Process monotonic native timestamps; a successful pair cannot be reused.
    pub fn observe(&mut self, event: TapEvent, now: Duration) -> bool {
        match event {
            TapEvent::KeyDown(key) => {
                self.keys.insert(key);
                self.first = None;
                self.down = None;
            }
            TapEvent::KeyUp(key) => {
                self.keys.remove(&key);
                self.first = None;
                self.down = None;
            }
            TapEvent::Cancel => {
                self.first = None;
                self.down = None;
                self.neutral = false;
            }
            TapEvent::Modifier {
                key,
                selected,
                alone,
                released,
            } => {
                if selected && alone && self.neutral && self.keys.is_empty() {
                    self.down = Some((key, now));
                    self.neutral = false;
                    return false;
                }
                let down = self.down.take();
                let eligible = selected && released && self.keys.is_empty();
                self.neutral = released;
                if eligible
                    && let Some((pressed, start)) = down
                    && pressed == key
                    && now.saturating_sub(start) <= self.interval
                {
                    if let Some(first) = self.first.take()
                        && now.saturating_sub(first) <= self.interval
                    {
                        return true;
                    }
                    self.first = Some(start);
                    return false;
                }
                self.first = None;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ctrl(down: bool) -> TapEvent {
        TapEvent::Modifier {
            key: 59,
            selected: true,
            alone: down,
            released: !down,
        }
    }
    fn run(events: &[(u64, TapEvent)]) -> usize {
        let mut gesture = DoubleTap::new(Duration::from_millis(400));
        events
            .iter()
            .filter(|(ms, event)| gesture.observe(*event, Duration::from_millis(*ms)))
            .count()
    }
    #[test]
    fn pair_completes_on_second_release_and_triple_does_not_reuse_it() {
        assert_eq!(
            run(&[(0, ctrl(true)), (30, ctrl(false)), (100, ctrl(true))]),
            0
        );
        assert_eq!(
            run(&[
                (0, ctrl(true)),
                (30, ctrl(false)),
                (100, ctrl(true)),
                (130, ctrl(false)),
                (200, ctrl(true)),
                (230, ctrl(false))
            ]),
            1
        );
        assert_eq!(run(&[(0, ctrl(true)), (30, ctrl(false))]), 0);
    }
    #[test]
    fn long_holds_timeouts_chords_and_intervening_keys_cancel() {
        for events in [
            vec![
                (0, ctrl(true)),
                (350, ctrl(false)),
                (360, ctrl(true)),
                (710, ctrl(false)),
            ],
            vec![
                (0, ctrl(true)),
                (500, ctrl(false)),
                (530, ctrl(true)),
                (560, ctrl(false)),
            ],
            vec![
                (0, ctrl(true)),
                (30, ctrl(false)),
                (500, ctrl(true)),
                (530, ctrl(false)),
            ],
            vec![
                (0, ctrl(true)),
                (10, TapEvent::KeyDown(8)),
                (20, TapEvent::KeyUp(8)),
                (30, ctrl(false)),
                (100, ctrl(true)),
                (130, ctrl(false)),
            ],
            vec![
                (0, ctrl(true)),
                (30, ctrl(false)),
                (60, TapEvent::KeyDown(8)),
                (70, TapEvent::KeyUp(8)),
                (100, ctrl(true)),
                (130, ctrl(false)),
            ],
            vec![
                (0, TapEvent::KeyDown(8)),
                (10, ctrl(true)),
                (20, TapEvent::KeyUp(8)),
                (30, ctrl(false)),
                (100, ctrl(true)),
                (130, ctrl(false)),
            ],
            vec![
                (0, ctrl(true)),
                (
                    10,
                    TapEvent::Modifier {
                        key: 56,
                        selected: false,
                        alone: false,
                        released: false,
                    },
                ),
                (20, ctrl(false)),
                (100, ctrl(true)),
                (130, ctrl(false)),
            ],
            vec![
                (0, ctrl(true)),
                (
                    10,
                    TapEvent::Modifier {
                        key: 62,
                        selected: true,
                        alone: true,
                        released: false,
                    },
                ),
                (30, ctrl(false)),
                (100, ctrl(true)),
                (130, ctrl(false)),
            ],
        ] {
            assert_eq!(run(&events), 0, "{events:?}");
        }
    }
}
