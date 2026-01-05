mod capnp ;
mod utils ;
mod initialisation ;
mod exports ;

use std::sync::Arc ;

use initialisation::{ InterfaceId, PluginId };
use initialisation::initialise_plugin_tree ;
pub use initialisation::PluginData ;



const ROOT_SOCKET_ID: &InterfaceId = &0x_00_00_00_00_u64 ;
const ROOT_SOCKET_INTERFACE: &str = "root:startup/root" ;
const STARTUP_FUNCTION: &str = "startup" ;

fn main() {

    let plugin_tree = match initialise_plugin_tree() {
        Ok( live_plugin_tree ) => Arc::new( live_plugin_tree ),
        Err( err ) => panic!( "Unrecoverable Startup Error: {}", err ),
    };

    let result = plugin_tree.dispatch_function_on_root( ROOT_SOCKET_INTERFACE, STARTUP_FUNCTION, false, &[] );
    println!( "{result:#?}" );

}
