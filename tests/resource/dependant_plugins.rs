use omni_desktop_host::initialise_plugin_tree ;
use wasmtime::component::Val ;

#[test]
fn resource_test_wrapper() {

    let ( tree, warnings ) = initialise_plugin_tree( &test_data_path!( "resource", "dependant_plugins" ), &0 ).unwrap();
    warnings.into_iter().for_each(| warning | println!( "{}", warning ));

    match tree.dispatch_function_on_root( "test:consumer/root", "get-value", true, &[] ) {
        omni_desktop_host::Socket::ExactlyOne( Ok( Val::U32( 42 ) )) => {}
        omni_desktop_host::Socket::ExactlyOne( Ok( val )) => panic!( "Expected U32(42), got: {:#?}", val ),
        omni_desktop_host::Socket::ExactlyOne( Err( err )) => panic!( "Method call failed: {:?}", err ),
        socket => panic!( "Expected ExactlyOne, got: {:#?}", socket ),
    }

}
