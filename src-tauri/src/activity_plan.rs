use rand::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Reading,
    Reviewing,
}

impl Scenario {
    pub fn label(self) -> &'static str {
        match self {
            Self::Reading => "Reading",
            Self::Reviewing => "Reviewing",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pause,
    Move,
    Scroll(i32),
    HorizontalScroll(i32),
}

pub struct Plan {
    pub scenario: Scenario,
    pub duration_ms: u64,
    pace: f64,
    last: Action,
}

impl Plan {
    pub fn new(rng: &mut impl Rng) -> Self {
        let scenario = if rng.random_bool(0.3) {
            Scenario::Reviewing
        } else {
            Scenario::Reading
        };
        Self {
            scenario,
            duration_ms: rng.random_range(90_000..=240_000),
            pace: rng.random_range(0.8..=1.3),
            last: Action::Pause,
        }
    }

    pub fn next(&mut self, rng: &mut impl Rng) -> (Action, u64) {
        self.pace = (self.pace * rng.random_range(0.94..=1.06)).clamp(0.7, 1.5);
        let roll = rng.random_range(0..100);
        let action = if matches!(self.last, Action::Scroll(_)) && rng.random_bool(0.45) {
            Action::Pause
        } else {
            match self.scenario {
                Scenario::Reviewing if roll < 15 => {
                    Action::HorizontalScroll(if rng.random_bool(0.5) { 1 } else { -1 })
                }
                _ => match roll {
                    0..=49 => Action::Scroll(if rng.random_bool(0.18) {
                        -rng.random_range(1..=2)
                    } else {
                        rng.random_range(1..=4)
                    }),
                    59..=75 => Action::Move,
                    _ => Action::Pause,
                },
            }
        };
        let pause = match action {
            Action::Pause if rng.random_bool(0.12) => rng.random_range(25_000..=75_000),
            Action::Pause => rng.random_range(5_000..=18_000),
            Action::Scroll(_) => rng.random_range(2_000..=10_000),
            _ => rng.random_range(700..=3_500),
        };
        self.last = action;
        (action, (pause as f64 * self.pace) as u64)
    }
}

pub fn pointer_path(
    start: (i32, i32),
    end: (i32, i32),
    bend: f64,
    steps: usize,
) -> Vec<(i32, i32)> {
    let (dx, dy) = ((end.0 - start.0) as f64, (end.1 - start.1) as f64);
    let control = (
        (start.0 as f64 + end.0 as f64) / 2.0 - dy * bend,
        (start.1 as f64 + end.1 as f64) / 2.0 + dx * bend,
    );
    (1..=steps)
        .map(|step| {
            let t = step as f64 / steps as f64;
            let t = t * t * (3.0 - 2.0 * t);
            let coordinate = |a: i32, b: f64, c: i32| {
                ((1.0 - t).powi(2) * a as f64 + 2.0 * (1.0 - t) * t * b + t * t * c as f64).round()
                    as i32
            };
            (
                coordinate(start.0, control.0, end.0),
                coordinate(start.1, control.1, end.1),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn plans_only_issue_reading_actions_with_positive_pauses() {
        let mut rng = StdRng::seed_from_u64(8);
        for _ in 0..60 {
            let mut plan = Plan::new(&mut rng);
            for _ in 0..200 {
                let (action, pause) = plan.next(&mut rng);
                assert!(pause > 0);
                assert!(matches!(
                    action,
                    Action::Pause | Action::Move | Action::Scroll(_) | Action::HorizontalScroll(_)
                ));
            }
        }
    }

    #[test]
    fn external_reading_never_issues_navigation_or_editing_keys() {
        let mut rng = StdRng::seed_from_u64(12);
        let mut plan = Plan::new(&mut rng);
        for _ in 0..1000 {
            let (action, _) = plan.next(&mut rng);
            assert!(matches!(
                action,
                Action::Pause | Action::Move | Action::Scroll(_) | Action::HorizontalScroll(_)
            ));
        }
    }

    #[test]
    fn scrolling_and_pauses_vary_without_mandatory_return_to_top() {
        let mut rng = StdRng::seed_from_u64(19);
        let mut plan = Plan::new(&mut rng);
        let actions: Vec<_> = (0..1000).map(|_| plan.next(&mut rng)).collect();
        assert!(actions
            .iter()
            .any(|(a, _)| matches!(a, Action::Scroll(v) if *v > 0)));
        assert!(actions
            .iter()
            .any(|(a, _)| matches!(a, Action::Scroll(v) if *v < 0)));
        assert!(actions.iter().any(|(_, pause)| *pause > 25_000));
        assert!(actions.iter().any(|(_, pause)| *pause < 2_000));
    }

    #[test]
    fn curved_cursor_path_reaches_its_target_and_eases_at_the_ends() {
        let path = pointer_path((0, 0), (1000, 0), 0.15, 40);
        assert_eq!(path.last(), Some(&(1000, 0)));
        assert!(path.iter().any(|point| point.1 > 0));
        assert!(path[0].0 < path[20].0 - path[19].0);
        assert!(1000 - path[38].0 < path[20].0 - path[19].0);
    }
}
