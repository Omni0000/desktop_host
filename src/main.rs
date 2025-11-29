
pub mod manifest_capnp {
	include!( concat!( env!( "OUT_DIR" ), "/manifest_capnp.rs" ));
}

mod startup ;
mod runtime ;

fn main() {

    if let Err( e ) = startup::startup() {
        eprintln!( "{}", e );
    };

}
