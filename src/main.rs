use crate::startup::startup;


pub mod manifest_capnp {
	include!( concat!( env!( "OUT_DIR" ), "/manifest_capnp.rs" ));
}

pub mod utils ;
mod startup ;
mod runtime ;

fn main() {

    match startup::startup() {
        Ok( mut tree ) => {
            tree.preload_socket( &"interface0".to_owned() ).unwrap();
        }
        Err( e ) => eprintln!( "{}", e ),
    };

}
