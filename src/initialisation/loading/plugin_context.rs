use wasmtime::component::ResourceTable ;
use wasmtime_wasi::{ WasiCtx, WasiCtxView, WasiView };

use super::RawPluginData ;



pub struct PluginContext {
    pub(super) data: RawPluginData,
    pub(super) wasi_ctx: WasiCtx,
    pub(super) wasi_table: ResourceTable,
}

impl PluginContext {
    pub fn new( data: RawPluginData ) -> Self {
        Self {
            data,
            wasi_ctx: WasiCtx::builder().build(),
            wasi_table: ResourceTable::new(),
        }
    }
}

impl std::fmt::Debug for PluginContext {
    fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct( "Plugin Instance" )
            .field( "data", &self.data )
            .field( "wasi_ctx", &"WasiCtx" )
            .field( "wasi_table", &"ResourceTable" )
            .finish()
    }
}

impl WasiView for PluginContext {
    fn ctx( &mut self ) -> wasmtime_wasi::WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi_ctx, table: &mut self.wasi_table }
    }
}

// TEMP: not Sync because of WasiCtx, no clue what to do about it for now
unsafe impl Sync for PluginContext {}
