use rand::Rng;

pub fn next_target<T: Clone + Eq>(
    remaining: &mut Vec<T>,
    available: &[T],
    current: Option<&T>,
    rng: &mut impl Rng,
) -> Option<T> {
    remaining.retain(|target| available.contains(target));
    if remaining.is_empty() {
        remaining.extend_from_slice(available);
    }
    let mut choices: Vec<_> = remaining
        .iter()
        .filter(|target| Some(*target) != current)
        .cloned()
        .collect();
    if choices.is_empty() && available.len() > 1 {
        remaining.clear();
        remaining.extend_from_slice(available);
        choices = remaining
            .iter()
            .filter(|target| Some(*target) != current)
            .cloned()
            .collect();
    }
    if choices.is_empty() {
        choices.clone_from(remaining);
    }
    let target = choices
        .get(rng.random_range(0..choices.len().max(1)))?
        .clone();
    remaining.retain(|item| *item != target);
    Some(target)
}

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
    can_scroll: bool,
    last: Action,
}

impl Plan {
    pub fn new(can_scroll: bool, rng: &mut impl Rng) -> Self {
        let scenario = if rng.random_bool(0.3) {
            Scenario::Reviewing
        } else {
            Scenario::Reading
        };
        Self {
            scenario,
            duration_ms: if can_scroll {
                rng.random_range(18_000..=40_000)
            } else {
                rng.random_range(8_000..=18_000)
            },
            can_scroll,
            pace: rng.random_range(0.8..=1.3),
            last: Action::Pause,
        }
    }

    pub fn next(&mut self, rng: &mut impl Rng) -> (Action, u64) {
        self.pace = (self.pace * rng.random_range(0.94..=1.06)).clamp(0.7, 1.5);
        let roll = rng.random_range(0..100);
        let action = if !self.can_scroll {
            if roll < 92 {
                Action::Move
            } else {
                Action::Pause
            }
        } else if matches!(self.last, Action::Scroll(_)) && rng.random_bool(0.45) {
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
        let pause = if !self.can_scroll {
            match action {
                Action::Pause => rng.random_range(180..=700),
                _ => rng.random_range(60..=280),
            }
        } else {
            match action {
                Action::Pause if rng.random_bool(0.12) => rng.random_range(25_000..=75_000),
                Action::Pause => rng.random_range(5_000..=18_000),
                Action::Scroll(_) => rng.random_range(2_000..=10_000),
                Action::Move => rng.random_range(100..=600),
                _ => rng.random_range(700..=3_500),
            }
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

pub fn pointer_route(
    bounds: crate::desktop::Bounds,
    rows: &[crate::desktop::Bounds],
    rng: &mut impl Rng,
) -> Vec<(i32, i32)> {
    use rand::seq::SliceRandom;
    let mut cells: Vec<_> = (0..9).collect();
    cells.shuffle(rng);
    cells
        .into_iter()
        .take(rng.random_range(3..=6))
        .map(|cell| {
            let left = bounds.x + 24 + ((bounds.width - 48) * (cell % 3) + 2) / 3;
            let right = bounds.x + 24 + ((bounds.width - 48) * (cell % 3 + 1) + 2) / 3;
            let top = bounds.y + 60 + ((bounds.height - 84) * (cell / 3) + 2) / 3;
            let bottom = bounds.y + 60 + ((bounds.height - 84) * (cell / 3 + 1) + 2) / 3;
            let candidates: Vec<_> = rows
                .iter()
                .map(|row| (row.x + row.width / 2, row.y + row.height / 2))
                .filter(|(x, y)| *x >= left && *x < right && *y >= top && *y < bottom)
                .collect();
            if !candidates.is_empty() && rng.random_bool(0.75) {
                candidates[rng.random_range(0..candidates.len())]
            } else {
                (rng.random_range(left..right), rng.random_range(top..bottom))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn windows_without_scroll_keep_moving_and_switch_within_eighteen_seconds() {
        let mut rng = StdRng::seed_from_u64(91);
        for _ in 0..30 {
            let mut plan = Plan::new(false, &mut rng);
            assert!((8_000..=18_000).contains(&plan.duration_ms));
            let actions: Vec<_> = (0..100).map(|_| plan.next(&mut rng)).collect();
            assert!(actions.iter().all(|(action, pause)| matches!(
                action,
                Action::Move | Action::Pause
            ) && *pause <= 1_050));
            assert!(
                actions
                    .iter()
                    .filter(|(action, _)| *action == Action::Move)
                    .count()
                    > 80
            );
        }
    }

    #[test]
    fn pointer_visits_multiple_distinct_regions_inside_the_window() {
        let mut rng = StdRng::seed_from_u64(61);
        let bounds = crate::desktop::Bounds {
            x: -800,
            y: 20,
            width: 790,
            height: 600,
        };
        for _ in 0..100 {
            let route = pointer_route(bounds, &[], &mut rng);
            assert!((3..=6).contains(&route.len()));
            assert!(route.iter().all(|(x, y)| *x >= bounds.x + 24
                && *x < bounds.x + bounds.width - 24
                && *y >= bounds.y + 60
                && *y < bounds.y + bounds.height - 24));
            let regions: std::collections::HashSet<_> = route
                .iter()
                .map(|(x, y)| {
                    (
                        (x - bounds.x - 24) * 3 / (bounds.width - 48),
                        (y - bounds.y - 60) * 3 / (bounds.height - 84),
                    )
                })
                .collect();
            assert_eq!(regions.len(), route.len());
        }
    }

    #[test]
    fn plans_only_issue_reading_actions_with_positive_pauses() {
        let mut rng = StdRng::seed_from_u64(8);
        for _ in 0..60 {
            let mut plan = Plan::new(true, &mut rng);
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
        let mut plan = Plan::new(true, &mut rng);
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
        let mut plan = Plan::new(true, &mut rng);
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
