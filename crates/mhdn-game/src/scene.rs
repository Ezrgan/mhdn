//! Quest scene, with three samples of hysteresis so a one-frame flicker does not reset the hunt.

/// How many identical raw samples a new scene must hold before it commits.
pub const HYSTERESIS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Disconnected,
    NoGame,
    Village,
    Loading,
    InQuest,
    QuestEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneChange {
    pub scene: Scene,
    /// Committed scene left `InQuest` on this sample.
    pub left_quest: bool,
}

#[derive(Debug, Clone)]
pub struct SceneMachine {
    committed: Scene,
    pending: Option<(Scene, u8)>,
}

impl Default for SceneMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneMachine {
    pub fn new() -> Self {
        Self {
            committed: Scene::Disconnected,
            pending: None,
        }
    }

    pub fn committed(&self) -> Scene {
        self.committed
    }

    pub fn observe(&mut self, in_quest: bool, loading: bool) -> SceneChange {
        let raw = classify(self.committed, in_quest, loading);
        self.push(raw)
    }

    pub fn disconnect(&mut self) -> SceneChange {
        self.push(Scene::Disconnected)
    }

    fn push(&mut self, raw: Scene) -> SceneChange {
        let before = self.committed;
        if raw == self.committed {
            self.pending = None;
        } else {
            match self.pending {
                Some((scene, seen)) if scene == raw => {
                    let seen = seen.saturating_add(1);
                    if seen >= HYSTERESIS {
                        self.committed = raw;
                        self.pending = None;
                    } else {
                        self.pending = Some((raw, seen));
                    }
                }
                _ => self.pending = Some((raw, 1)),
            }
        }
        SceneChange {
            scene: self.committed,
            left_quest: before == Scene::InQuest && self.committed != Scene::InQuest,
        }
    }
}

/// The reward screen shares the loading byte. Leaving a hunt into that byte is quest end.
pub fn classify(previous: Scene, in_quest: bool, loading: bool) -> Scene {
    if in_quest && !loading {
        Scene::InQuest
    } else if loading && matches!(previous, Scene::InQuest | Scene::QuestEnd) {
        Scene::QuestEnd
    } else if loading {
        Scene::Loading
    } else {
        Scene::Village
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(machine: &mut SceneMachine, in_quest: bool, loading: bool, times: u8) -> SceneChange {
        let mut change = machine.observe(in_quest, loading);
        for _ in 1..times {
            change = machine.observe(in_quest, loading);
        }
        change
    }

    #[test]
    fn village_load_and_hunt_need_three_samples() {
        let mut machine = SceneMachine::new();
        assert_eq!(
            hold(&mut machine, false, false, 2).scene,
            Scene::Disconnected
        );
        assert_eq!(hold(&mut machine, false, false, 1).scene, Scene::Village);
        assert_eq!(hold(&mut machine, false, true, 3).scene, Scene::Loading);
        assert_eq!(hold(&mut machine, true, false, 3).scene, Scene::InQuest);
    }

    #[test]
    fn reward_screen_after_a_hunt_is_quest_end() {
        let mut machine = SceneMachine::new();
        hold(&mut machine, true, false, 3);
        let end = hold(&mut machine, false, true, 3);
        assert_eq!(end.scene, Scene::QuestEnd);
        assert!(end.left_quest);
        assert_eq!(hold(&mut machine, false, false, 3).scene, Scene::Village);
    }

    #[test]
    fn one_loading_sample_does_not_drop_the_hunt() {
        let mut machine = SceneMachine::new();
        hold(&mut machine, true, false, 3);
        let flicker = machine.observe(false, true);
        assert_eq!(flicker.scene, Scene::InQuest);
        assert!(!flicker.left_quest);
    }
}
