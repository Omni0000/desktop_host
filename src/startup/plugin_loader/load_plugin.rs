use thiserror::Error ;
use wasmtime::{ Engine, Instance, Module, Store};

use crate::startup::Plugin ;
use super::ActivePlugin ;




#[derive( Error, Debug )]
pub enum LoaderError {

    #[error( "Wasmtime Error: {0}" )]
    WasmtimeError( #[from] wasmtime::Error ),

    #[error( "Capnp Error: {0}" )]
    CapnpError( #[from] capnp::Error ),

    #[error( "Utf8Error: {0}" )]
    Utf8Error( #[from] std::str::Utf8Error ),

}

pub fn load_plugin<'a>( engine: &Engine, plugin: Plugin ) -> Result<ActivePlugin, LoaderError> {
    
    let module = Module::from_file( engine, plugin.wasm() )?;
    let mut store = Store::new( engine, ());
    let instance = Instance::new( &mut store, &module, &[])?;

    Ok( ActivePlugin {
        id: plugin.id().to_owned(),
        plugin,
        module,
        store,
        instance,
    })

}