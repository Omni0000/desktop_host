use omni_desktop_host::{ initialise_plugin_tree, UnrecoverableStartupError, PreloadError, InterfaceCardinality };

#[test]
fn cardinality_test_exactly_one_with_none() {
    match initialise_plugin_tree( &test_data_path!( "cardinality", "exactly_one", "with_none" ), &0 ) {
        Err(( UnrecoverableStartupError::PreloadError(
            PreloadError::FailedCardinalityRequirements( InterfaceCardinality::ExactlyOne, 0 )
        ), _ )) => {},
        Err(( err, warnings )) if warnings.is_empty() => panic!( "{}", err ),
        Err(( err, warnings )) => panic!( "Failed With Warnings: {}\n{:?}", err, warnings ),
        Ok( val ) => panic!( "Expected failure, got: {:#?}", val ),
    };
}
