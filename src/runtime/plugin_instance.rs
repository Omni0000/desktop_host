
use crate::{ plugin_decoder::PluginId, plugin_parser::PluginNode };

pub struct PluginInstance {
    pub plugin_data: PluginNode,
    pub instance: wasmtime::Instance,
    pub store: wasmtime::Store<PluginHostState>,
}

pub struct PluginHostState {
    pub plugin_id: PluginId,
}