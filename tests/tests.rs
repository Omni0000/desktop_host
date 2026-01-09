use omni_desktop_host::initialise_plugin_tree ;

macro_rules! test_data_path {
    ($( $segment:expr ),+ $(,)?) => {{
        std::path::PathBuf::from( env!( "CARGO_MANIFEST_DIR" ))
            .join( "tests" )
            $(.join( $segment ))+
    }};
}

#[test]
fn load_root() {
    initialise_plugin_tree( &test_data_path!( "load_root" ), &0 ).unwrap();
}
