use omni_desktop_host::{ initialise_plugin_tree, UnrecoverableStartupError, PreloadError, InterfaceCardinality };

#[test]
fn cardinality_test_at_least_one_with_none() {
    match initialise_plugin_tree( &test_data_path!( "cardinality", "at_least_one", "with_none" ), &0 ) {
        Err( UnrecoverableStartupError::PluginPreloadError(
            PreloadError::FailedCardinalityRequirements( InterfaceCardinality::AtLeastOne, 0 )
        )) => {},
        Err( err ) => panic!( "{}", err ),
        Ok( val ) => panic!( "Expected failure, got: {:#?}", val ),
    };
}
