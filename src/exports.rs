use wasmtime::{ Engine, Linker };

use crate::startup::Plugin;

mod bridge ;



macro_rules! declare_exports {
    (
        $linker_instance:expr,
        [
            $(( $name:literal, $function:expr )),*
            $(,)?
        ]
    ) => {
        vec![ $( $linker_instance.func_wrap( "env", $name, $function ).err() ),* ]
            .into_iter()
            .filter_map(|x| x)
            .collect::<Vec<_>>()
    };
}

pub fn exports( engine: &Engine ) -> ( Linker<Plugin>, Vec<wasmtime::Error> ) {

    let mut linker = Linker::new( &engine );
    let linker_errors = declare_exports!( linker, [
        ( "call_on_socket", bridge::call_on_socket ),
    ]);

    ( linker, linker_errors )

}