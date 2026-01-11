use omni_desktop_host::initialise_plugin_tree ;
use wasmtime::component::Val ;

#[test]
fn dispatch_test_dependant_plugins_expect_composite() {
    let tree = initialise_plugin_tree( &test_data_path!( "dispatching", "dependant_plugins_expect_composite" ), &0,).unwrap();
    match tree.dispatch_function_on_root( "test:dependant-composite/root", "get-composite", true, &[] ) {
        omni_desktop_host::Socket::ExactlyOne( Ok( Val::Tuple( fields ) )) => {
            assert_eq!( fields[0], Val::U32( 42 ) );
            assert_eq!( fields[1], Val::U32( 24 ) );
        }
        value => panic!( "Expected ExactlyOne( Ok( Tuple( ... ))), found: {value:#?}" ),
    }
}
