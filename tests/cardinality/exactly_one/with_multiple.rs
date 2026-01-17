use omni_desktop_host::{ initialise_plugin_tree, InterfaceCardinality, PreloadError, UnrecoverableStartupError };

#[test]
fn cardinality_test_exactly_one_with_multiple() {
    match initialise_plugin_tree( &test_data_path!( "cardinality", "exactly_one", "with_multiple" ), &0 ) {
        Err(( UnrecoverableStartupError::PreloadError(
            PreloadError::FailedCardinalityRequirements( InterfaceCardinality::ExactlyOne, n )
        ), _ )) if n > 1 => {}
        Err(( err, warnings )) if warnings.is_empty() => panic!( "{}", err ),
        Err(( err, warnings )) => panic!( "Failed With Warnings: {}\n{:?}", err, warnings ),
        value => panic!( "Expected PluginPreloadError( FailedCardinalityRequirements( ExactlyOne, 0 )), found: {value:#?}" ),
    }
}
