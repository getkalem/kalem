//! Kalem's change to xkbcommon 0.8.0 (README.md): libxkbcommon and
//! libxkbcommon-x11 are opened the first time one of their functions is
//! called (`dlopen`) instead of being linked, so that a program built with
//! this crate starts on a system that has neither and needs them only once
//! it calls into them.

use std::ffi::{c_void, CStr};

/// Opens the library `name` (a soname such as `libxkbcommon.so.0`) for
/// the rest of the program's life, or panics with the loader's reason.
pub(crate) fn open(name: &CStr) -> *mut c_void {
    // SAFETY: `name` is a C string; the handle is never closed, so the
    // functions taken from it stay valid.
    let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        panic!(
            "xkbcommon: cannot load {}: {}",
            name.to_string_lossy(),
            last_error()
        );
    }
    handle
}

/// The address of the function `name` in `handle`, null when the library
/// has none (an older libxkbcommon lacks the newer functions).
pub(crate) fn symbol(handle: *mut c_void, name: &CStr) -> *mut c_void {
    // SAFETY: `handle` came from `open` and is never closed.
    unsafe { libc::dlsym(handle, name.as_ptr()) }
}

fn last_error() -> String {
    // SAFETY: dlerror returns null or a C string valid until the next
    // dl call on this thread, copied at once.
    let e = unsafe { libc::dlerror() };
    if e.is_null() {
        "unknown error".into()
    } else {
        unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
    }
}

/// Declares the functions of a library opened at run time with the
/// signatures of an `extern "C"` block: each becomes an `unsafe fn` of the
/// same name and signature that opens the library on the first call and
/// calls the library's function, panicking when the library cannot be
/// opened or lacks the function.
macro_rules! functions {
    (
        library = $soname:literal;
        $(
            pub fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;
        )*
    ) => {
        #[allow(non_snake_case)]
        struct Functions {
            $( $name: Option<unsafe extern "C" fn($($ty),*) $(-> $ret)?>, )*
        }

        fn functions() -> &'static Functions {
            static FUNCTIONS: std::sync::OnceLock<Functions> = std::sync::OnceLock::new();
            FUNCTIONS.get_or_init(|| {
                let handle = $crate::dl::open(
                    std::ffi::CStr::from_bytes_with_nul(concat!($soname, "\0").as_bytes()).unwrap(),
                );
                Functions {
                    $(
                        // SAFETY: the symbol is the C function of this
                        // signature (libxkbcommon's headers), or null,
                        // which is `None`.
                        $name: unsafe {
                            std::mem::transmute::<
                                *mut std::ffi::c_void,
                                Option<unsafe extern "C" fn($($ty),*) $(-> $ret)?>,
                            >($crate::dl::symbol(
                                handle,
                                std::ffi::CStr::from_bytes_with_nul(
                                    concat!(stringify!($name), "\0").as_bytes(),
                                )
                                .unwrap(),
                            ))
                        },
                    )*
                }
            })
        }

        $(
            #[inline]
            pub unsafe fn $name($($arg: $ty),*) $(-> $ret)? {
                match functions().$name {
                    Some(f) => f($($arg),*),
                    None => panic!(concat!("xkbcommon: ", $soname, " has no ", stringify!($name))),
                }
            }
        )*
    };
}

pub(crate) use functions;
