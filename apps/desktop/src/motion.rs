static SYSTEM_REDUCE_MOTION: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn system_reduce_motion() -> bool {
    *SYSTEM_REDUCE_MOTION.get_or_init(read_system_reduce_motion)
}

#[cfg(target_os = "macos")]
fn read_system_reduce_motion() -> bool {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    let Some(class) = AnyClass::get(c"NSWorkspace") else {
        return false;
    };
    let workspace: *mut AnyObject = unsafe { msg_send![class, sharedWorkspace] };
    if workspace.is_null() {
        return false;
    }
    let reduce: bool = unsafe { msg_send![workspace, accessibilityDisplayShouldReduceMotion] };
    reduce
}

#[cfg(not(target_os = "macos"))]
fn read_system_reduce_motion() -> bool {
    false
}
