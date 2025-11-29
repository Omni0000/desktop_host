use wasmtime::{ Module, Store, Instance };
use crate::startup::Plugin;
use crate::startup::plugin_deserialiser::PluginId ;

pub struct ActivePlugin {
    pub(super) id: PluginId,
    pub(super) plugin: Plugin,
    pub(super) module: Module,
    pub(super) store: Store<()>,
    pub(super) instance: Instance,
}