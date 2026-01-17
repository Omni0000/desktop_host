use pipe_trait::Pipe ;

use omni_desktop_host::initialise_plugin_tree ;
use omni_desktop_host::utils::deconstruct_partial_result ;

#[test]
fn cardinality_test_at_least_one_with_one() {
 
    let ( result, warnings ) = initialise_plugin_tree( &test_data_path!( "cardinality", "at_least_one", "with_one" ), &0 )
        .pipe( deconstruct_partial_result );

    match result {
        Ok(_) if warnings.is_empty() => {},
        Ok(_) => panic!( "Produced Warnings: {:?}", warnings ),
        Err( err ) if warnings.is_empty() => panic!( "{}", err ),
        Err( err ) => panic!( "Failed with warnings: {}\n{:?}", err, warnings ),
    }

}
