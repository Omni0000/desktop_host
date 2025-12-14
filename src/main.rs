mod capnp ;
mod utils ;
mod startup ;
mod exports ;

use std::sync::Arc ;
use lazy_static::lazy_static ;

use startup::InterfaceId;
use startup::{ startup, LivePluginTree };




lazy_static! {
    static ref LIVE_PLUGIN_TREE: Arc<LivePluginTree> = {
        match startup() {
            Ok( live_plugin_tree ) => Arc::new( live_plugin_tree ),
            Err( err ) => panic!( "Startup Error: {}", err ),
        }
    };
}



const ROOT_SOCKET_ID: &'static InterfaceId = &0x_00_00_00_00_u64 ;
const STARTUP_FUNCTION: &'static str = "startup" ;

fn main() {

    match LIVE_PLUGIN_TREE.dispatch_function( ROOT_SOCKET_ID, STARTUP_FUNCTION, &[] ) {
        Ok( res ) => println!( "res: {:#?}", res ),
        Err( err ) => eprintln!( "Error: {}", err ),
    };

}
