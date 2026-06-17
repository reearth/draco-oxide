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
/// Behaves like `debug_assert!` (active under `cfg(debug_assertions)`) but can
/// additionally be forced on in release builds by enabling the `safety_assertions`
/// feature. Default behavior is therefore unchanged: on in debug, off in release.
///
/// Like `debug_write!`/`debug_expect!`, the `feature = "safety_assertions"` cfg
/// resolves at the call site, so every crate invoking this macro must declare the
/// `safety_assertions` feature (forwarding into core).
#[macro_export]
macro_rules! safety_assert {
    ($($arg:tt)*) => {
        #[cfg(any(debug_assertions, feature = "safety_assertions"))]
        {
            ::core::assert!($($arg)*);
        }
    };
}

/// `assert_eq!` counterpart of [`safety_assert!`]; see that macro for semantics.
#[macro_export]
macro_rules! safety_assert_eq {
    ($($arg:tt)*) => {
        #[cfg(any(debug_assertions, feature = "safety_assertions"))]
        {
            ::core::assert_eq!($($arg)*);
        }
    };
}
