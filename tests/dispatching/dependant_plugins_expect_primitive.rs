use omni_desktop_host::initialise_plugin_tree ;
use wasmtime::component::Val ;

#[test]
fn dispatch_test_dependant_plugins_expect_primitive() {
    let tree = initialise_plugin_tree( &test_data_path!( "dispatching", "dependant_plugins_expect_primitive" ), &0,).unwrap();
    match tree.dispatch_function_on_root( "test:dependant-primitive/root", "get-primitive", true, &[] ) {
        omni_desktop_host::Socket::ExactlyOne( Ok( Val::U32( 42 ) )) => {}
        value => panic!( "Expected ExactlyOne( Ok( U32( 42 ))), found: {value:#?}" ),
    }
}
