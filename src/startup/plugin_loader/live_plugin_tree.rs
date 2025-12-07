use std::collections::HashMap ;
use wasmtime::{ Engine, Linker };

use crate::startup::Plugin ; 
use super::super::plugin_preprocessor::SocketMap ;
use super::ActivePlugin ;

pub struct LivePluginTree {
    pub(super) engine: Engine,
    pub(super) socket_map: SocketMap<PluginTreeNode>,
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

impl From<Plugin> for PluginTreeNode {
    fn from( plugin: Plugin ) -> Self { Self::LazyPlugin( plugin ) }
}

#[inline] fn map_socket_map( socket_map: SocketMap<Plugin> ) -> SocketMap<PluginTreeNode> {
    socket_map
        .into_iter()
        .map(|( key, plugins_vec )| (
            key,
            plugins_vec.into_iter().map( PluginTreeNode::from ).collect(),
        ))
        .collect::<HashMap<_, _>>()
}