use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn cardinality_test_exactly_one_with_one() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "cardinality", "exactly_one", "with_one" ), &0 ) { panic!( "{err}" )};
}
