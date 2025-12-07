
pub mod manifest_capnp {
	include!( concat!( env!( "OUT_DIR" ), "/manifest_capnp.rs" ));
}

pub mod utils ;
mod startup ;
mod exports ;

use startup::FunctionDispatchInstruction ;

fn main() {

    match startup::startup() {
        Ok( mut tree ) => {
            
            let errors = tree.preload_socket( &"00000000".to_owned() ).unwrap();
            if errors.len() > 0 { eprintln!( "{:?}", errors );}
            
            let data = 257i32.to_ne_bytes();
            println!( "data: {:?}", data );
            match tree.dispatch_function(
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

            match tree.dispatch_function(
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
        Err( e ) => eprintln!( "{}", e ),
    };

}
