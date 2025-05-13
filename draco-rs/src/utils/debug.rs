#[macro_export]
macro_rules! debug_write {
    ($msg:literal, $writer:expr) => {
        #[cfg(debug_assertions)]
        {
            for byte in $msg.as_bytes() {
                $writer((8, *byte as u64));
            }
        }
    };
}

#[macro_export]
macro_rules! debug_expect {
    ($msg:literal, $stream_in:expr) => {
        #[cfg(debug_assertions)]
        {
            for byte in $msg.as_bytes() {
                assert!(
                    *byte == $stream_in(8) as u8,
                    "Expected {:?}, but did not match.",
                    $msg
                );
            }
        }
    };
}