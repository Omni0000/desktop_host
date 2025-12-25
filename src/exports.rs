use wasmtime::Engine ;
use wasmtime::component::Linker ;

use crate::initialisation::PluginContext ;



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

pub fn exports( engine: &Engine ) -> ( Linker<PluginContext>, Vec<wasmtime::Error> ) {

    let mut linker = Linker::new( &engine );
    wasmtime_wasi::p2::add_to_linker_sync( &mut linker ).expect( "TEMP: wasi p2 linking failure" );
    let linker_errors = declare_exports!( linker, [
    ]);

    ( linker, linker_errors )

}