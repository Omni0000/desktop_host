use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn cardinality_test_at_least_one_with_multiple() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "cardinality", "at_least_one", "with_multiple" ), &0 ) { panic!( "{err}" )};
}
