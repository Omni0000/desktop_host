use omni_desktop_host::{ initialise_plugin_tree, UnrecoverableStartupError, PreloadError, InterfaceCardinality };

#[test]
fn cardinality_test_at_most_one_with_multiple() {
    match initialise_plugin_tree( &test_data_path!( "cardinality", "at_most_one", "with_multiple" ), &0 ) {
        Err(( UnrecoverableStartupError::PreloadError(
            PreloadError::FailedCardinalityRequirements( InterfaceCardinality::AtMostOne, n )
        ), _ )) if n > 1 => {},
        Err(( err, warnings )) if warnings.is_empty() => panic!( "{}", err ),
        Err(( err, warnings )) => panic!( "Failed With Warnings: {}\n{:?}", err, warnings ),
        Ok( val ) => panic!( "Expected failure, got: {:#?}", val ),
    };
}
