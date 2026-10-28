pub trait Keyboard {
    /// `None` where the platform cannot report modifier state.
    fn alt_held(&self) -> Option<bool>;
}

pub struct SystemKeyboard;

#[cfg(target_os = "macos")]
mod macos {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    const COMBINED_SESSION_STATE: i32 = 0;
    const ALTERNATE_FLAG: u64 = 0x0008_0000;

    pub fn alt_held() -> bool {
        // SAFETY: reads the session's modifier flags; takes and keeps nothing.
        let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) };
        flags & ALTERNATE_FLAG != 0
    }
}

impl Keyboard for SystemKeyboard {
    #[cfg(target_os = "macos")]
    fn alt_held(&self) -> Option<bool> {
        Some(macos::alt_held())
    }

    #[cfg(not(target_os = "macos"))]
    fn alt_held(&self) -> Option<bool> {
        None
    }
}
