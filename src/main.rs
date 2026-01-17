use std::path::PathBuf ;
use std::sync::Arc ;
use pipe_trait::Pipe ;

use omni_desktop_host::{ InterfaceId, initialise_plugin_tree };
use omni_desktop_host::utils::{ deconstruct_partial_result, produce_warning };



const SOURCE_DIR: &str = "./appdata" ;
const ROOT_SOCKET_ID: InterfaceId = 0x_00_00_00_00_u64 ;
const ROOT_SOCKET_INTERFACE: &str = "root:startup/root" ;
const STARTUP_FUNCTION: &str = "startup" ;

fn main() {

    let ( init_result, init_errors ) = initialise_plugin_tree( &PathBuf::from( SOURCE_DIR ), &ROOT_SOCKET_ID )
        .pipe( deconstruct_partial_result );

    init_errors.into_iter().for_each( produce_warning );

    let plugin_tree = match init_result {
        Ok( live_plugin_tree ) => Arc::new( live_plugin_tree ),
        Err( err ) => panic!( "Unrecoverable Startup Error: {}", err ),
    };

    let result = plugin_tree.dispatch_function_on_root( ROOT_SOCKET_INTERFACE, STARTUP_FUNCTION, false, &[] );
    println!( "{:#?}", result );

}
