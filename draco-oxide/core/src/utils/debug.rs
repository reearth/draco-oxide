#[macro_export]
macro_rules! debug_write {
    ($msg:literal, $writer:expr) => {
        #[cfg(feature = "debug_format")]
        {
            for byte in $msg.as_bytes() {
                $writer.write_u8(*byte);
            }
        }
    };
}

#[macro_export]
macro_rules! debug_expect {
    ($msg:literal, $reader:expr) => {
        #[cfg(feature = "debug_format")]
        {
            for byte in $msg.as_bytes() {
                assert!(
                    *byte == $reader.read_u8().unwrap(),
                    "Expected {:?}, but did not match.",
                    $msg
                );
            }
        }
    };
}

/// Safety assertion guarding `unsafe` preconditions and internal invariants.
///
/// A failing safety assertion indicates a bug in this library, never a runtime or
/// input condition; it is not a hardening check. The checks run only in debug
/// builds, and only while the (default-on) `safety_assertions` feature is enabled
/// — i.e. `all(debug_assertions, feature = "safety_assertions")`. Release builds
/// never run them, and disabling the feature (`--no-default-features`) drops them
/// from debug builds too.
///
/// Like `debug_write!`/`debug_expect!`, the `feature = "safety_assertions"` cfg
/// resolves at the call site, so every crate invoking this macro must declare the
/// (default-on) `safety_assertions` feature (forwarding into core).
#[macro_export]
macro_rules! safety_assert {
    ($($arg:tt)*) => {
        #[cfg(all(debug_assertions, feature = "safety_assertions"))]
        {
            ::core::assert!($($arg)*);
        }
    };
}

/// `assert_eq!` counterpart of [`safety_assert!`]; see that macro for semantics.
#[macro_export]
macro_rules! safety_assert_eq {
    ($($arg:tt)*) => {
        #[cfg(all(debug_assertions, feature = "safety_assertions"))]
        {
            ::core::assert_eq!($($arg)*);
        }
    };
}
