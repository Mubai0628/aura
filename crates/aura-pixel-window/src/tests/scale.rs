use crate::scale::{fitting_scale, whole_scale};

#[test]
fn whole_scales_are_exact_or_the_nearest_multiple() {
    assert_eq!(whole_scale(96), 1, "100 %");
    assert_eq!(whole_scale(120), 1, "125 % draws at 100 %");
    assert_eq!(whole_scale(144), 2, "150 % draws at 200 %");
    assert_eq!(whole_scale(168), 2, "175 %");
    assert_eq!(whole_scale(192), 2, "200 %");
    assert_eq!(whole_scale(288), 3, "300 %");
    assert_eq!(whole_scale(72), 1, "never below 100 %");
}

#[test]
fn a_window_too_large_for_the_work_area_is_drawn_a_step_smaller() {
    let setup = (760, 435);
    assert_eq!(fitting_scale(192, setup, (3840, 2100)), 2);
    assert_eq!(fitting_scale(192, setup, (1280, 680)), 1, "1532 px does not fit 1280");
    assert_eq!(fitting_scale(288, setup, (2560, 1400)), 3, "2298 x 1323 px fits 2560 x 1400");
    assert_eq!(
        fitting_scale(288, setup, (2560, 1300)),
        2,
        "1323 px does not fit 1300: 300 % falls to 200 %"
    );
    assert_eq!(fitting_scale(96, setup, (640, 400)), 1, "100 % is the floor");
}
