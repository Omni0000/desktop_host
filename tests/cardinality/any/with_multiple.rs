use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn cardinality_test_any_with_multiple() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "cardinality", "any", "with_multiple" ), &0 ) { panic!( "{err}" )};
}
