use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn loading_test_dependant_plugins() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "loading", "dependant_plugins" ), &0 ) {
        panic!( "{err}" )
    };
}
