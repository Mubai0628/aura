//! One held-button gesture, with its input boundary supplied by the desktop driver.
use aura_desktop_domain::{Delivery, HostError, HostErrorCode, Point};

pub trait Driver {
    /// Takes the pointer to `point`; an error when the user took the desktop over on the way.
    fn move_to(&mut self, point: Point) -> Result<bool, HostError>;
    fn press(&mut self) -> bool;
    fn release(&mut self) -> bool;
    fn checkpoint(&mut self) -> Result<(), HostError>;
    fn check_point(&mut self, point: Point) -> Result<(), HostError>;
}

pub fn run(driver: &mut impl Driver, path: &[Point], done: u32) -> Result<bool, HostError> {
    let Some(first) = path.first() else {
        return Err(HostError::new(
            HostErrorCode::Refused,
            "拖动路径为空，没有执行",
            Delivery::NotDelivered,
        ));
    };
    driver.check_point(*first)?;
    if !driver.move_to(*first)? {
        return Ok(false);
    }
    let pressed = driver.press();
    let gesture = (|| {
        if !pressed {
            return Ok(false);
        }
        for point in &path[1..] {
            driver.checkpoint()?;
            driver.check_point(*point)?;
            if !driver.move_to(*point)? {
                return Ok(false);
            }
        }
        Ok(true)
    })();
    // Every path after a possible button-down releases it exactly once, including refusals.
    let released = driver.release();
    match gesture {
        Err(error) => {
            Err(HostError { delivery: Delivery::Uncertain, steps_done: Some(done), ..error })
        }
        Ok(true) if released => Ok(true),
        Ok(_) => Err(HostError {
            code: HostErrorCode::Refused,
            message: "拖动没有完整送达 Windows，已尝试松开鼠标；这一步结果不明，不要重做".into(),
            delivery: Delivery::Uncertain,
            steps_done: Some(done),
        }),
    }
}
