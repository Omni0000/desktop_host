macro_rules! include_capnp {
    
    ($parent_name:ident { $($child_items:tt)* } $(,)*) => {
        pub mod $parent_name {
            include_capnp! { @inner_path_builder [$parent_name] $($child_items)* }
        }
        include_capnp! { @create_root_re_exports $parent_name ; $($child_items)* }
    };

    ($parent_name:ident { $($child_items:tt)* } , $($rest:tt)*) => {
        pub mod $parent_name {
            include_capnp! { @inner_path_builder [$parent_name] $($child_items)* }
        }
        include_capnp! { @create_root_re_exports $parent_name ; $($child_items)* }
        include_capnp! { $($rest)* }
    };

    ($name:ident $(,)*) => {
        paste::paste! {
            pub mod [<$name _capnp>] {
                include!( concat!( env!( "OUT_DIR" ), "/capnp/", stringify!( $name ), "_capnp.rs" ));
            }
        }
    };

    ($item:tt , $($rest:tt)*) => {
        include_capnp! { $item }
        include_capnp! { $($rest)* }
    };

    (@inner_path_builder [$($path_segments:ident)+] $name:ident $(,)*) => {
        paste::paste! {
            pub mod [<$name _capnp>] {
                include!( concat!( env!( "OUT_DIR" ), "/capnp/", $( stringify!( $path_segments ), "/", )* stringify!( $name ), "_capnp.rs" ) );
            }
        }
    };

    (@inner_path_builder [$($path_segments:ident)+] $name:ident { $($inner_content:tt)* } $(,)*) => {
        pub mod $name {
            include_capnp! { @inner_path_builder [$($path_segments)* $name] $($inner_content)* }
        }
    };

    (@inner_path_builder [$($path_segments:ident)+] $item:tt , $($rest:tt)*) => {
        include_capnp! { @inner_path_builder [$($path_segments)*] $item }
        include_capnp! { @inner_path_builder [$($path_segments)*] $($rest)* }
    };

    (@create_root_re_exports $parent_name:ident ; $child_name:ident $(,)*) => {
        paste::paste! {
            mod [<$child_name _capnp>] {
                #[allow(unused_imports)]
                pub use super::$parent_name::[<$child_name _capnp>]::*;
            }
        }
    };

    (@create_root_re_exports $parent_name:ident ; $child_name:ident { $($inner_content:tt)* } $(,)*) => {
        paste::paste! {
            mod [<$child_name _capnp>] {
                #[allow(unused_imports)]
                pub use super::$parent_name::[<$child_name _capnp>]::*;
            }
        }
    };

    (@create_root_re_exports $parent_name:ident ; $item:tt , $($rest:tt)*) => {
        include_capnp! { @create_root_re_exports $parent_name ; $item }
        include_capnp! { @create_root_re_exports $parent_name ; $($rest)* }
    };
}

include_capnp! {
    common {
        interface,
        plugin,
        version,
    },
    manifest {
        interface_manifest,
        plugin_manifest,
    },
}