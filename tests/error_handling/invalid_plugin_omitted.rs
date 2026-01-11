use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn error_handling_test_invalid_plugin_omitted() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "error_handling", "invalid_plugin_omitted" ), &0 ) {
        panic!("{err}")
    };
}
