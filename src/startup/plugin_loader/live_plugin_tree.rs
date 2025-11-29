use std::collections::HashMap ;
use wasmtime::Engine ;

use crate::startup::{ Plugin, InterfaceId }; 
use super::ActivePlugin ;

pub struct LivePluginTree {
    pub(super) engine: Engine,
    pub(super) socket_map: HashMap<InterfaceId, Vec<PluginTreeNode> >,
}

impl LivePluginTree {
    pub fn new(
        engine: Engine,
        socket_map: HashMap<InterfaceId, Vec<Plugin>>,
    ) -> Self {
        Self {
            engine,
            socket_map: socket_map
                .into_iter()
                .map(|( key, plugins_vec )| {
                    (
                        key,
                        plugins_vec
                            .into_iter()
                            .map( PluginTreeNode::from )
                            .collect(),
                    )
                })
                .collect(),
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