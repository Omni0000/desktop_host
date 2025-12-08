use std::collections::HashMap ;
use std::sync::RwLock ;
use wasmtime::{ Engine, Linker };

use crate::startup::Plugin ; 
use super::super::plugin_preprocessor::SocketMap ;
use super::ActivePlugin ;

pub struct LivePluginTree {
    pub(super) engine: Engine,
    pub(super) socket_map: SocketMap<RwLock<PluginTreeNode>>,
    pub(super) linker: Linker<Plugin>,
}

impl LivePluginTree {
    pub fn new(
        engine: Engine,
        socket_map: SocketMap<Plugin>,
        linker: Linker<Plugin>,
    ) -> Self {
        Self {
            engine,
            socket_map: map_socket_map( socket_map ),
            linker,
        }
    }
}

pub enum PluginTreeNode {
    ActivePlugin( ActivePlugin ),
    LazyPlugin( Plugin ),
}

impl From<Plugin> for RwLock<PluginTreeNode> {
    fn from( plugin: Plugin ) -> Self { RwLock::new( PluginTreeNode::LazyPlugin( plugin ))}
}

#[inline] fn map_socket_map( socket_map: SocketMap<Plugin> ) -> SocketMap<RwLock<PluginTreeNode>> {
    socket_map
        .into_iter()
        .map(|( key, plugins_map )| (
            key,
            plugins_map
                .into_iter()
                .map(|( plugin_id, plugin )| ( plugin_id, RwLock::<PluginTreeNode>::from( plugin ) ))
                .collect(),
        ))
        .collect::<HashMap<_, _>>()
}