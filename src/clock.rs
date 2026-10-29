use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock {
    fn now_millis(&self) -> u64;
    fn pause(&self, millis: u64);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    }

    fn pause(&self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}
