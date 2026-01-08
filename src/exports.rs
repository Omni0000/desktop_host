use wasmtime::component::Linker;
use wasmtime::Engine;

use crate::initialisation::PluginContext;

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

pub fn exports(engine: &Engine) -> (Linker<PluginContext>, Vec<wasmtime::Error>) {
    let linker = Linker::new(&engine);
    let linker_errors = declare_exports!(linker, []);

    (linker, linker_errors)
}
