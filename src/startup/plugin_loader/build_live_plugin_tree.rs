use std::collections::HashMap ;
use wasmtime::Engine ;

use crate::startup::{ InterfaceId, Plugin };
use super::LivePluginTree ;



pub fn build_live_plugin_tree<'a>( socket_map: HashMap<InterfaceId, Vec<Plugin>> ) -> LivePluginTree {

    let engine = Engine::default();

    LivePluginTree::new( engine, socket_map )

}