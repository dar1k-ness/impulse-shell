//! Bit-flag sets numbered by hand. Two branches that each add a flag with the same number merge
//! without a conflict; the checks here stop that tree from building.

#[doc(hidden)]
pub use bitflags;

/// Declares the flags of one set as written (free constants, `impl Type { .. }` or the body of
/// a `bitflags!` struct) and fails the build unless each is a single bit no other flag of the
/// set has.
#[macro_export]
macro_rules! bit_flags {
    (
        $(#[$($sm:tt)*])* $sv:vis struct $ty:ident: $t:ty {
            $($(#[$($m:tt)*])* const $name:ident = $val:expr;)+
        }
    ) => {
        $crate::bitflags::bitflags! {
            $(#[$($sm)*])* $sv struct $ty: $t {
                $($(#[$($m)*])* const $name = $val;)+
            }
        }
        const _: () = {
            $(const $name: $t = $ty::$name.bits();)+
            $crate::bit_flags!(@distinct $($name)+);
        };
    };
    (impl $ty:ty { $($(#[$m:meta])* $v:vis const $name:ident: $t:ty = $val:expr;)+ }) => {
        impl $ty {
            $($(#[$m])* $v const $name: $t = $val;)+
        }
        const _: () = {
            $(const $name: $t = <$ty>::$name;)+
            $crate::bit_flags!(@distinct $($name)+);
        };
    };
    ($($(#[$m:meta])* $v:vis const $name:ident: $t:ty = $val:expr;)+) => {
        $($(#[$m])* $v const $name: $t = $val;)+
        const _: () = {
            $crate::bit_flags!(@distinct $($name)+);
        };
    };
    (@distinct $first:ident $($rest:ident)*) => {
        assert!(
            ($first as u64).is_power_of_two(),
            concat!(stringify!($first), " is not a single bit")
        );
        $(assert!(
            $first != $rest,
            concat!(stringify!($first), " and ", stringify!($rest), " are the same bit")
        );)*
        $crate::bit_flags!(@distinct $($rest)*);
    };
    (@distinct) => {};
}
