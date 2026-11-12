//! macOS: which app opens `mailto:` links. OxyMail declares the scheme in its Info.plist, and
//! Launch Services keeps the user's choice.
//!
//! Kept free of the rest of the app so it can be type-checked for macOS from any machine.

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSCopyDefaultHandlerForURLScheme(scheme: CFStringRef) -> CFStringRef;
    fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, handler: CFStringRef) -> i32;
}

/// Whether the app with this bundle identifier opens `mailto:` links.
pub fn is_default_mail_app(bundle_id: &str) -> bool {
    let scheme = CFString::new("mailto");
    // SAFETY: a valid CFString in; the result is a +1 reference (or null), owned below.
    let handler = unsafe { LSCopyDefaultHandlerForURLScheme(scheme.as_concrete_TypeRef()) };
    if handler.is_null() {
        return false;
    }
    // SAFETY: "Copy" in the name means we own this reference.
    let handler = unsafe { CFString::wrap_under_create_rule(handler) };
    handler.to_string().eq_ignore_ascii_case(bundle_id)
}

/// Makes the app with this bundle identifier open `mailto:` links. Newer macOS versions may
/// ask the user to confirm.
pub fn make_default_mail_app(bundle_id: &str) -> Result<(), String> {
    let scheme = CFString::new("mailto");
    let handler = CFString::new(bundle_id);
    // SAFETY: both arguments are valid CFStrings for the length of the call.
    let status = unsafe { LSSetDefaultHandlerForURLScheme(scheme.as_concrete_TypeRef(), handler.as_concrete_TypeRef()) };
    if status == 0 { Ok(()) } else { Err(format!("macOS refused the change (error {status})")) }
}
