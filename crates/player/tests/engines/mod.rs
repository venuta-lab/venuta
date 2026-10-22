//! The player suite runs on both engines while both exist (section 9.1 of the Listen together
//! design): `on_both_engines!` makes each test twice, `name::engine_1` and `name::engine_2`.

#![allow(dead_code)]

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
                run(player::engine::EngineKind::One)
            }

            #[test]
            $(#[$m])*
            fn engine_2() {
                run(player::engine::EngineKind::Two)
            }
        }
        $crate::on_both_engines! { $($rest)* }
    };
    () => {};
}
