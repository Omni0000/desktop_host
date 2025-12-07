use wasmtime::{ Engine, Linker, Module, Store };

use crate::startup::Plugin ;
use super::ActivePlugin ;



#[inline] pub(super) fn load_plugin(
    engine: &Engine,
    plugin: Plugin,
    linker: &Linker<Plugin>
) -> Result<ActivePlugin, wasmtime::Error> {
    
    let id = plugin.id().to_owned();

    let module = Module::from_file( engine, plugin.wasm() )?;
    let mut store = Store::new( engine, plugin );
    let instance = linker.instantiate( &mut store, &module )?;

    Ok( ActivePlugin {
        _id: id,
        _module: module,
        store,
        instance,
    })

}