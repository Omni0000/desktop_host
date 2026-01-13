use omni_desktop_host::initialise_plugin_tree ;
use wasmtime::component::Val ;

#[test]
fn resource_test_method_call() {

    let tree = initialise_plugin_tree( &test_data_path!( "resource", "single_plugin" ), &0 ).unwrap();

    let resource_handle = match tree.dispatch_function_on_root( "test:myresource/root", "[constructor]counter", true, &[] ) {
        omni_desktop_host::Socket::ExactlyOne( Ok( Val::Resource( handle ) )) => handle,
        omni_desktop_host::Socket::ExactlyOne( Ok( val )) => panic!( "Expected resource, got: {val:#?}" ),
        omni_desktop_host::Socket::ExactlyOne( Err( e )) => panic!( "Constructor failed: {e:?}" ),
        socket => panic!( "Expected ExactlyOne, got: {socket:#?}" ),
    };

    match tree.dispatch_function_on_root( "test:myresource/root", "[method]counter.get-value", true, &[Val::Resource( resource_handle )] ) {
        omni_desktop_host::Socket::ExactlyOne( Ok( Val::U32( 42 ) )) => {}
        omni_desktop_host::Socket::ExactlyOne( Ok( val )) => panic!( "Expected U32(42), got: {val:#?}" ),
        omni_desktop_host::Socket::ExactlyOne( Err( e )) => panic!( "Method call failed: {e:?}" ),
        socket => panic!( "Expected ExactlyOne, got: {socket:#?}" ),
    }

}
