use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn cardinality_test_any_with_none() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "cardinality", "any", "with_none" ), &0 ) { panic!( "{err}" )};
}
