//! Typed config key constants.
//!
//! Use these instead of raw string literals:
//! ```
//! use crate::config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk};
//! config.get_bool(bk::SHOW_SWAP);
//! config.get_int(ik::UPDATE_MS);
//! config.get_string(sk::COLOR_THEME);
//! ```

pub use crate::config::{bool_keys, int_keys, str_keys};
