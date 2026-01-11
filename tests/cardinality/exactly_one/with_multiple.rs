use omni_desktop_host::{ initialise_plugin_tree, InterfaceCardinality, PreloadError, UnrecoverableStartupError };

#[test]
fn cardinality_test_exactly_one_with_multiple() {
    match initialise_plugin_tree( &test_data_path!( "cardinality", "exactly_one", "with_multiple" ), &0 ) {
        Err( UnrecoverableStartupError::PluginPreloadError(
            PreloadError::FailedCardinalityRequirements( InterfaceCardinality::ExactlyOne, n )
        )) if n > 1 => {}
        Err( err ) => panic!( "{}", err ),
        value => panic!( "Expected PluginPreloadError( FailedCardinalityRequirements( ExactlyOne, 0 )), found: {value:#?}" ),
    }
}
