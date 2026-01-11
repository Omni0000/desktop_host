use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn loading_test_single_plugin() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "loading", "single_plugin" ), &0 ) {
        panic!( "{err}" )
    };
}
