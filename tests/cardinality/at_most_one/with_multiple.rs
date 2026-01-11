use omni_desktop_host::{ initialise_plugin_tree, UnrecoverableStartupError, PreloadError, InterfaceCardinality };

#[test]
fn cardinality_test_at_most_one_with_multiple() {
    match initialise_plugin_tree( &test_data_path!( "cardinality", "at_most_one", "with_multiple" ), &0 ) {
        Err( UnrecoverableStartupError::PluginPreloadError(
            PreloadError::FailedCardinalityRequirements( InterfaceCardinality::AtMostOne, n )
        )) if n > 1 => {},
        Err( err ) => panic!( "{}", err ),
        Ok( val ) => panic!( "Expected failure, got: {:#?}", val ),
    };
}
