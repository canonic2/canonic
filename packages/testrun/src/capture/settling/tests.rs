//! Deterministic action-observation timing and visual detection regressions.
use super::*;

fn sample(shade: u8) -> Sample {
    Sample(GrayImage::from_pixel(60, 130, image::Luma([shade])))
}

#[test]
fn a_quiet_old_screen_waits_for_delayed_change_then_stillness() {
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut wait = Settling::new(Some(sample(68)), start);
    assert!(!wait.sample(sample(68), at(0)));
    assert!(!wait.sample(sample(68), at(300)));
    assert!(!wait.sample(sample(68), at(1300)));
    assert_eq!(wait.remaining(at(1300)), Duration::from_millis(700));
    assert!(!wait.sample(sample(187), at(1400)));
    assert_eq!(wait.remaining(at(1400)), STILL_LIMIT);
    assert!(!wait.sample(sample(187), at(1699)));
    assert!(wait.sample(sample(187), at(1700)));
    let status = wait.status(at(1700));
    assert_eq!(status.change_detected, Some(true));
    assert!(status.settled);
}

#[test]
fn unchanged_and_small_changes_expire_without_claiming_detection() {
    for small in [false, true] {
        let start = Instant::now();
        let mut wait = Settling::new(Some(sample(68)), start);
        assert!(!wait.sample(sample(68), start));
        for ms in [800, 1100, 1900] {
            let mut current = sample(68);
            if small {
                current.0.put_pixel(30, 65, image::Luma([255]));
            }
            assert!(!wait.sample(current, start + Duration::from_millis(ms)));
        }
        let deadline = start + CHANGE_LIMIT;
        assert!(wait.remaining(deadline).is_zero());
        let status = wait.status(deadline);
        assert_eq!(status.change_detected, Some(false));
        assert!(status.settled);
        // A late decoded frame cannot retroactively satisfy the change window.
        assert!(!wait.sample(sample(187), deadline));
        assert_eq!(wait.status(deadline).change_detected, Some(false));
    }
}

#[test]
fn motion_has_a_separate_budget_and_does_not_claim_stillness() {
    let start = Instant::now();
    let mut wait = Settling::new(Some(sample(68)), start);
    for ms in (1400..=2800).step_by(100) {
        let shade = if ms % 200 == 0 { 187 } else { 20 };
        assert!(!wait.sample(sample(shade), start + Duration::from_millis(ms)));
    }
    let deadline = start + Duration::from_millis(2900);
    assert!(wait.remaining(deadline).is_zero());
    let status = wait.status(deadline);
    assert_eq!(status.change_detected, Some(true));
    assert!(!status.settled);
}

#[test]
fn observations_without_input_do_not_claim_a_change_measurement() {
    let start = Instant::now();
    let mut wait = Settling::new(None, start);
    assert!(!wait.sample(sample(68), start));
    let at = start + QUIET;
    assert!(wait.sample(sample(68), at));
    assert_eq!(wait.status(at).change_detected, None);
    assert!(Sample::decode(b"invalid frame").is_err());
}

#[test]
fn immediate_change_and_a_delayed_animation_require_their_own_quiet_period() {
    let start = Instant::now();
    let mut immediate = Settling::new(Some(sample(68)), start);
    assert!(!immediate.sample(sample(187), start));
    assert!(immediate.sample(sample(187), start + QUIET));

    let mut animated = Settling::new(Some(sample(68)), start);
    assert!(!animated.sample(sample(68), start + Duration::from_millis(300)));
    for (ms, shade) in [(1400, 80), (1600, 110), (1800, 150), (2000, 187)] {
        assert!(!animated.sample(sample(shade), start + Duration::from_millis(ms)));
    }
    assert!(!animated.sample(sample(187), start + Duration::from_millis(2299)));
    assert!(animated.sample(sample(187), start + Duration::from_millis(2300)));
}
