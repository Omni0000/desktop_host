use wasmtime::{ Engine, Linker};

use crate::startup::Plugin;

mod test_exports ;



macro_rules! declare_exports {
    (
        $linker_instance:expr,
        [
            $(( $module:literal, $name:literal, $function:expr )),*
            $(,)?
        ]
    ) => {
        vec![ $( $linker_instance.func_wrap( $module, $name, $function ).err() ),* ]
            .into_iter()
            .filter_map(|x| x)
            .collect::<Vec<_>>()
    };
}

pub fn exports( engine: &Engine ) -> ( Linker<Plugin>, Vec<wasmtime::Error> ) {

    let mut linker = Linker::new( &engine );
    let linker_errors = declare_exports!( linker, [
        ( "env", "add_one", test_exports::add_one ),
        ( "env", "print_to_host", test_exports::print_to_host ),
    ]);

    ( linker, linker_errors )

}