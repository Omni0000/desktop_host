use omni_desktop_host::initialise_plugin_tree ;

#[test]
fn cardinality_test_at_most_one_with_none() {
    if let Err( err ) = initialise_plugin_tree( &test_data_path!( "cardinality", "at_most_one", "with_none" ), &0 ) { panic!( "{err}" )};
}
