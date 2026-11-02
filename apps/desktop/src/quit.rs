use std::ffi::CStr;
use std::path::{Path, PathBuf};

pub const QUIT_SELECTOR: &CStr = c"terminate:";

pub const CLOSE_SELECTOR: &CStr = c"performClose:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitRouting {
    Routed,

    Unsupported,

    NoMenu,

    NoQuitItem,
}

impl QuitRouting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Routed => "routed",
            Self::Unsupported => "unsupported",
            Self::NoMenu => "no-menu",
            Self::NoQuitItem => "no-quit-item",
        }
    }

    pub fn is_graceful(self) -> bool {
        matches!(self, Self::Routed | Self::Unsupported)
    }
}

impl std::fmt::Display for QuitRouting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn quit_item_index(actions: &[Option<&CStr>]) -> Option<usize> {
    actions
        .iter()
        .position(|action| *action == Some(QUIT_SELECTOR))
}

pub const CHECK_ENV: &str = "SCREENROOM_QUIT_CHECK";

pub const TERMINATE_ENV: &str = "SCREENROOM_TERMINATE_CHECK";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminateRouting {
    Routed,

    Unsupported,

    NoApplication,

    NoMethod,
}

impl TerminateRouting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Routed => "routed",
            Self::Unsupported => "unsupported",
            Self::NoApplication => "no-application",
            Self::NoMethod => "no-method",
        }
    }

    pub fn is_routed(self) -> bool {
        matches!(self, Self::Routed)
    }
}

impl std::fmt::Display for TerminateRouting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn check_path() -> Option<PathBuf> {
    std::env::var_os(CHECK_ENV).map(PathBuf::from)
}

pub fn terminate_check_path() -> Option<PathBuf> {
    std::env::var_os(TERMINATE_ENV).map(PathBuf::from)
}

pub fn terminate_check_requested() -> bool {
    terminate_check_path().is_some()
}

pub fn install_terminate_routing() -> TerminateRouting {
    #[cfg(target_os = "macos")]
    {
        imp::install_terminate()
    }
    #[cfg(not(target_os = "macos"))]
    {
        TerminateRouting::Unsupported
    }
}

pub fn request_termination() {
    #[cfg(target_os = "macos")]
    unsafe {
        imp::terminate_application()
    }
}

pub fn check_requested() -> bool {
    check_path().is_some()
}

pub fn report_check(path: &Path, routing: QuitRouting) -> std::io::Result<()> {
    std::fs::write(path, format!("{routing}\n"))
}

pub fn route_quit_through_close() -> QuitRouting {
    #[cfg(target_os = "macos")]
    {
        imp::route()
    }
    #[cfg(not(target_os = "macos"))]
    {
        QuitRouting::Unsupported
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicPtr, Ordering};

    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};

    use super::{CLOSE_SELECTOR, QuitRouting, TerminateRouting, quit_item_index};

    static PREVIOUS_TERMINATE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    unsafe fn find_quit_item(menu: *mut AnyObject) -> Option<*mut AnyObject> {
        if let Some(item) = unsafe { item_that_quits(menu) } {
            return Some(item);
        }
        let count: isize = unsafe { msg_send![menu, numberOfItems] };
        for index in 0..count {
            let item: *mut AnyObject = unsafe { msg_send![menu, itemAtIndex: index] };
            if item.is_null() {
                continue;
            }
            let submenu: *mut AnyObject = unsafe { msg_send![item, submenu] };
            if submenu.is_null() {
                continue;
            }
            if let Some(item) = unsafe { item_that_quits(submenu) } {
                return Some(item);
            }
        }
        None
    }

    unsafe fn item_that_quits(menu: *mut AnyObject) -> Option<*mut AnyObject> {
        let index = quit_item_index(&unsafe { item_actions(menu) })?;
        let item: *mut AnyObject = unsafe { msg_send![menu, itemAtIndex: index as isize] };
        (!item.is_null()).then_some(item)
    }

    unsafe fn item_actions(menu: *mut AnyObject) -> Vec<Option<&'static std::ffi::CStr>> {
        let count: isize = unsafe { msg_send![menu, numberOfItems] };
        let mut actions = Vec::with_capacity(count.max(0) as usize);
        for index in 0..count {
            let item: *mut AnyObject = unsafe { msg_send![menu, itemAtIndex: index] };

            let action: Option<Sel> = if item.is_null() {
                None
            } else {
                unsafe { msg_send![item, action] }
            };
            actions.push(action.map(Sel::name));
        }
        actions
    }

    pub fn route() -> QuitRouting {
        let Some(application_class) = AnyClass::get(c"NSApplication") else {
            return QuitRouting::NoMenu;
        };

        let application: *mut AnyObject =
            unsafe { msg_send![application_class, sharedApplication] };
        if application.is_null() {
            return QuitRouting::NoMenu;
        }
        let menu: *mut AnyObject = unsafe { msg_send![application, mainMenu] };
        if menu.is_null() {
            return QuitRouting::NoMenu;
        }
        let Some(quit) = (unsafe { find_quit_item(menu) }) else {
            return QuitRouting::NoQuitItem;
        };

        let close = Sel::register(CLOSE_SELECTOR);
        let no_target: *mut AnyObject = std::ptr::null_mut();
        unsafe {
            let _: () = msg_send![quit, setAction: close];
            let _: () = msg_send![quit, setTarget: no_target];
        }
        QuitRouting::Routed
    }

    unsafe fn application() -> *mut AnyObject {
        let Some(class) = AnyClass::get(c"NSApplication") else {
            return std::ptr::null_mut();
        };
        unsafe { msg_send![class, sharedApplication] }
    }

    unsafe fn closable(window: *mut AnyObject) -> bool {
        if window.is_null() {
            return false;
        }
        let close = Sel::register(CLOSE_SELECTOR);
        let responds: objc2::runtime::Bool =
            unsafe { msg_send![window, respondsToSelector: close] };
        responds.as_bool()
    }

    unsafe fn close_target(application: *mut AnyObject) -> *mut AnyObject {
        let key: *mut AnyObject = unsafe { msg_send![application, keyWindow] };
        if unsafe { closable(key) } {
            return key;
        }
        let main: *mut AnyObject = unsafe { msg_send![application, mainWindow] };
        if unsafe { closable(main) } {
            return main;
        }
        let windows: *mut AnyObject = unsafe { msg_send![application, windows] };
        if windows.is_null() {
            return std::ptr::null_mut();
        }
        let count: isize = unsafe { msg_send![windows, count] };
        for index in 0..count {
            let window: *mut AnyObject = unsafe { msg_send![windows, objectAtIndex: index] };
            if unsafe { closable(window) } {
                return window;
            }
        }
        std::ptr::null_mut()
    }

    unsafe fn forward_termination(application: *mut AnyObject, command: Sel) {
        let previous = PREVIOUS_TERMINATE.load(Ordering::Acquire);
        if previous.is_null() {
            return;
        }
        let previous: unsafe extern "C-unwind" fn(*mut AnyObject, Sel) =
            unsafe { std::mem::transmute(previous) };
        unsafe { previous(application, command) };
    }

    unsafe extern "C-unwind" fn close_instead_of_terminating(
        application: *mut AnyObject,
        command: Sel,
    ) {
        let window = unsafe { close_target(application) };
        if window.is_null() {
            unsafe { forward_termination(application, command) };
            return;
        }
        let no_sender: *mut AnyObject = std::ptr::null_mut();
        let _: () = unsafe { msg_send![window, performClose: no_sender] };
    }

    pub fn install_terminate() -> TerminateRouting {
        let Some(class) = AnyClass::get(c"NSApplication") else {
            return TerminateRouting::NoApplication;
        };
        let Some(method) = class.instance_method(Sel::register(c"terminate:")) else {
            return TerminateRouting::NoMethod;
        };
        let replacement: Imp = unsafe {
            std::mem::transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel), Imp>(
                close_instead_of_terminating,
            )
        };
        let previous = unsafe { method.set_implementation(replacement) };
        PREVIOUS_TERMINATE.store(previous as *const () as *mut c_void, Ordering::Release);
        TerminateRouting::Routed
    }

    pub unsafe fn terminate_application() {
        let application = unsafe { application() };
        if application.is_null() {
            return;
        }
        let no_sender: *mut AnyObject = std::ptr::null_mut();
        let _: () = unsafe { msg_send![application, terminate: no_sender] };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quit_item_is_found_by_its_action_not_by_its_place() {
        let about = c"orderFrontStandardAboutPanel:";
        let hide = c"hide:";

        let application_menu = [Some(about), None, None, Some(hide), Some(QUIT_SELECTOR)];
        assert_eq!(quit_item_index(&application_menu), Some(4));

        let other_menu = [Some(about), None, Some(hide)];
        assert_eq!(quit_item_index(&other_menu), None);
        assert_eq!(quit_item_index(&[]), None);
        assert_eq!(quit_item_index(&[None, None]), None);
    }

    #[test]
    fn the_replacement_is_the_close_button_s_action() {
        assert_eq!(CLOSE_SELECTOR.to_bytes(), b"performClose:");
        assert_eq!(QUIT_SELECTOR.to_bytes(), b"terminate:");
        assert_ne!(quit_item_index(&[Some(CLOSE_SELECTOR)]), Some(0));
    }

    #[test]
    fn only_a_menu_that_was_not_routed_is_reported_as_lossy() {
        assert!(QuitRouting::Routed.is_graceful());
        assert!(QuitRouting::Unsupported.is_graceful());
        assert!(!QuitRouting::NoMenu.is_graceful());
        assert!(!QuitRouting::NoQuitItem.is_graceful());
    }

    #[test]
    fn terminate_routing_is_only_settled_when_the_method_was_replaced() {
        assert!(TerminateRouting::Routed.is_routed());
        for failure in [
            TerminateRouting::Unsupported,
            TerminateRouting::NoApplication,
            TerminateRouting::NoMethod,
        ] {
            assert!(!failure.is_routed(), "{failure} must not read as routed");
        }
        assert_eq!(TerminateRouting::NoMethod.to_string(), "no-method");
    }

    #[test]
    fn the_terminate_check_reports_through_its_own_environment_variable() {
        assert_eq!(TERMINATE_ENV, "SCREENROOM_TERMINATE_CHECK");
        assert_ne!(TERMINATE_ENV, CHECK_ENV);
    }
}
