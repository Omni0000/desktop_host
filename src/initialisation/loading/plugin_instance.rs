use wasmtime::component::{ Component, Instance };
use wasmtime::Store ;

use crate::initialisation::loading::PluginContext;

use super::super::PluginId ;



pub struct PluginInstance {
    pub(super) _id: PluginId,
    pub(super) _component: Component,
    pub(super) store: Store<PluginContext>,
    pub(super) instance: Instance,
}

impl std::fmt::Debug for PluginInstance {
    fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct( "Plugin Instance" )
            .field( "id", &self._id )
            .field( "data", &self.store.data() )
            .field( "store", &self.store )
            .finish()
    }
}