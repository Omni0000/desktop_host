use wasmtime::{ Module, Store, Instance };
use crate::startup::Plugin;
use crate::startup::plugin_deserialiser::PluginId ;

pub struct ActivePlugin {
    pub(super) _id: PluginId,
    pub(super) _module: Module,
    pub(super) store: Store<Plugin>,
    pub(super) instance: Instance,
}