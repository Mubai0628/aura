//! A recording driver exercises the production gesture loop without touching the desktop.
use aura_desktop_control::drag::{self, Driver};
use aura_desktop_control::guard::{Gate, Input, State};
use aura_desktop_domain::{Delivery, HostError, HostErrorCode, Point};

struct Recording {
    state: State,
    at: Point,
    held: bool,
    /// The user presses a mouse button at this checkpoint, from the first (0).
    user_at: Option<usize>,
    /// What of the user's comes on the way to the start.
    user_on_the_way: Option<Input>,
    checkpoints: usize,
    refuse_endpoint: bool,
    fail_endpoint: bool,
    fail_release: bool,
    log: Vec<(&'static str, Point)>,
}

impl Recording {
    fn new() -> Self {
        Self {
            state: State::new(),
            at: Point { x: 0, y: 0 },
            held: false,
            user_at: None,
            user_on_the_way: None,
            checkpoints: 0,
            refuse_endpoint: false,
            fail_endpoint: false,
            fail_release: false,
            log: vec![],
        }
    }

    fn gate(&self) -> Result<(), HostError> {
        match self.state.gate() {
            Gate::Ready => Ok(()),
            Gate::TakenOver => Err(HostError {
                code: HostErrorCode::TakenOver,
                message: "用户已接管电脑".into(),
                delivery: Delivery::Uncertain,
                steps_done: Some(2),
            }),
        }
    }
}

impl Driver for Recording {
    fn move_to(&mut self, point: Point) -> Result<bool, HostError> {
        if let Some(input) = self.user_on_the_way
            && !self.held
        {
            self.state.observe(input, true);
            self.log.push(("user", Point { x: 400, y: 400 }));
        }
        self.gate()?;
        if self.fail_endpoint && point == path()[2] {
            return Ok(false);
        }
        self.at = point;
        self.state.observe(Input::Aura, true);
        self.log.push(("move", point));
        Ok(true)
    }
    fn press(&mut self) -> bool {
        assert!(!self.held, "a partially completed gesture must not be replayed");
        self.held = true;
        self.log.push(("down", self.at));
        true
    }
    fn release(&mut self) -> bool {
        self.held = false;
        self.log.push(("up", self.at));
        !self.fail_release
    }
    fn checkpoint(&mut self) -> Result<(), HostError> {
        if self.user_at == Some(self.checkpoints) {
            self.state.observe(Input::Button { injected: false }, true);
            self.log.push(("user-button", self.at));
        }
        self.checkpoints += 1;
        self.gate()
    }
    fn check_point(&mut self, point: Point) -> Result<(), HostError> {
        if self.refuse_endpoint && point == path()[2] {
            return Err(HostError::new(
                HostErrorCode::Refused,
                "target changed",
                Delivery::NotDelivered,
            ));
        }
        Ok(())
    }
}

fn path() -> [Point; 3] {
    [Point { x: 0, y: 0 }, Point { x: 100, y: 0 }, Point { x: 200, y: 0 }]
}

#[test]
fn a_whole_drag_presses_once_and_lets_go_at_its_end() {
    let mut driver = Recording::new();
    assert!(drag::run(&mut driver, &path(), 2).expect("dragged"));
    assert_eq!(driver.log.iter().filter(|(kind, _)| *kind == "down").count(), 1);
    assert_eq!(driver.log.last(), Some(&("up", path()[2])));
    assert!(!driver.held);
}

#[test]
fn the_user_taking_over_during_a_drag_lets_the_button_go_and_goes_no_further() {
    let mut driver = Recording::new();
    driver.user_at = Some(1);
    let error = drag::run(&mut driver, &path(), 2).expect_err("taken over");
    assert_eq!(error.code, HostErrorCode::TakenOver);
    assert_eq!((error.delivery, error.steps_done), (Delivery::Uncertain, Some(2)));
    assert_eq!(driver.log.last().expect("released").0, "up");
    assert!(!driver.log.iter().any(|(kind, at)| *kind == "move" && *at == path()[2]));
    assert!(!driver.held);
}

#[test]
fn taking_over_on_the_way_to_the_start_never_presses_the_button() {
    let mut driver = Recording::new();
    driver.user_on_the_way = Some(Input::Wheel { injected: false });
    let error = drag::run(&mut driver, &path(), 2).expect_err("taken over");
    assert_eq!(error.code, HostErrorCode::TakenOver);
    assert!(
        !driver.log.iter().any(|(kind, _)| *kind == "down" || *kind == "up"),
        "{:?}",
        driver.log
    );
}

#[test]
fn a_move_of_the_users_on_the_way_is_no_takeover_and_the_drag_goes_on() {
    let mut driver = Recording::new();
    driver.user_on_the_way = Some(Input::Ignored);
    assert!(drag::run(&mut driver, &path(), 2).expect("dragged"));
    assert_eq!(driver.log.last(), Some(&("up", path()[2])));
}

#[test]
fn the_endpoint_is_checked_before_the_pointer_goes_there_and_a_refusal_lets_the_button_go() {
    let mut driver = Recording::new();
    driver.refuse_endpoint = true;
    let error = drag::run(&mut driver, &path(), 2).expect_err("target must be checked");
    assert_eq!(error.delivery, Delivery::Uncertain);
    assert_eq!(driver.log.last().expect("released").0, "up");
    assert!(!driver.log.iter().any(|(kind, at)| *kind == "move" && *at == path()[2]));
}

#[test]
fn a_failed_move_after_button_down_is_uncertain_and_always_released() {
    let mut driver = Recording::new();
    driver.fail_endpoint = true;
    let error = drag::run(&mut driver, &path(), 2)
        .expect_err("partial input cannot be called not executed");
    assert_eq!(error.delivery, Delivery::Uncertain);
    assert_eq!(error.steps_done, Some(2));
    assert_eq!(driver.log.last().expect("released").0, "up");
    assert!(!driver.held);
}

#[test]
fn a_release_windows_did_not_take_is_uncertain_even_after_the_whole_path() {
    let mut driver = Recording::new();
    driver.fail_release = true;
    let error = drag::run(&mut driver, &path(), 2).expect_err("the button may still be down");
    assert_eq!(error.delivery, Delivery::Uncertain);
    assert_eq!(error.steps_done, Some(2));
    assert_eq!(driver.log.iter().filter(|(kind, _)| *kind == "move").count(), path().len());
}

#[test]
fn an_empty_path_is_refused_before_anything_moves() {
    let mut driver = Recording::new();
    let error = drag::run(&mut driver, &[], 0).expect_err("nothing to drag");
    assert_eq!((error.code, error.delivery), (HostErrorCode::Refused, Delivery::NotDelivered));
    assert!(driver.log.is_empty());
}
