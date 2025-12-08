mod capnp ;
mod utils ;
mod startup ;
mod exports ;

use std::sync::Arc ;
use lazy_static::lazy_static ;

use startup::{ startup, LivePluginTree };
use startup::FunctionDispatchInstruction ;



lazy_static! {
    static ref LIVE_PLUGIN_TREE: Arc<LivePluginTree> = {
        match startup() {
            Ok( live_plugin_tree ) => Arc::new( live_plugin_tree ),
            Err( err ) => panic!( "Startup Error: {}", err ),
        }
    };
}



fn main() {

    let data = 257i32.to_ne_bytes();
    println!( "data: {:?}", data );
    match LIVE_PLUGIN_TREE.dispatch_function(
        FunctionDispatchInstruction::new( "00000000".to_string(), "wasm_add_one".to_string() ),
        &data,
    ) {
        Ok(( results, errors )) => {
            if errors.len() > 0 { eprintln!( "{:?}", errors );}
            for result in results {
                match result {
                    Ok( res ) => println!( "res: {:?}", res ),
                    Err( e ) => eprintln!( "fail: {}", e ),
                }
            }
        }
        Err( e ) => eprintln!( "{}", e )
    };

    match LIVE_PLUGIN_TREE.dispatch_function(
        FunctionDispatchInstruction::new( "00000000".to_string(), "print".to_string() ),
        &[],
    ) {
        Ok(( results, errors )) => {
            if errors.len() > 0 { eprintln!( "{:?}", errors );}
            for result in results {
                match result {
                    Ok( res ) => println!( "res: {:?}", res ),
                    Err( e ) => eprintln!( "fail: {}", e ),
                }
            }
        },
        Err( e ) => eprintln!( "{}", e )
    }

}
