//! The player suite runs on both engines while both exist (section 9.1 of the Listen together
//! design): `on_both_engines!` makes each test twice, `name::engine_1` and `name::engine_2`.
//!
//! Each plays a real player against a device that takes sound at real-time pace, so each has the
//! computer to itself, as a player does: the tests of a binary take turns (`real_time`) instead of
//! running side by side, where a dozen real-time players on a CI runner's few cores would starve
//! one another and a device would run dry for that alone.

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard};

static REAL_TIME: Mutex<()> = Mutex::new(());

/// The computer to this test alone while it plays in real time (a test that failed before keeps
/// no one else waiting).
pub fn real_time() -> MutexGuard<'static, ()> {
    REAL_TIME.lock().unwrap_or_else(|p| p.into_inner())
}

/// `on_both_engines! { fn name(engine) { body } ... }`: for each function, a module `name` with
/// the tests `engine_1` and `engine_2`, each running the body with `engine` the engine it names.
/// Attributes before `fn` (an `#[ignore]`) go on both.
#[macro_export]
macro_rules! on_both_engines {
    ($(#[$m:meta])* fn $name:ident($e:ident) $body:block $($rest:tt)*) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            fn run($e: player::engine::EngineKind) $body

            #[test]
            $(#[$m])*
            fn engine_1() {
                let _turn = $crate::engines::real_time();
                run(player::engine::EngineKind::One)
            }

            #[test]
            $(#[$m])*
            fn engine_2() {
                let _turn = $crate::engines::real_time();
                run(player::engine::EngineKind::Two)
            }
        }
        $crate::on_both_engines! { $($rest)* }
    };
    () => {};
}
