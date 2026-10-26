// Only Android calls into this cdylib (`android_main`). Built for any other
// target it has no entry point, and every item would be reported as unused.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

include!("main.rs");
