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