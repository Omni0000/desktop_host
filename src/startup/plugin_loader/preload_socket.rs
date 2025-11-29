use wasmtime::Engine ;
use itertools::Itertools ;

use crate::startup::{ InterfaceId };
use super::{ LivePluginTree, PluginTreeNode };
use super::{ load_plugin, LoaderError };



#[derive( Debug )]
pub struct InvalidSocket( pub InterfaceId );

impl std::error::Error for InvalidSocket {}
impl std::fmt::Display for InvalidSocket {
    fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
        write!( f, "Socket '{}' not found", self.0 )
    }
}

impl LivePluginTree {
    pub fn preload_socket(
        &mut self,
        socket: &InterfaceId,
    ) -> Result<Vec<LoaderError>, InvalidSocket> {
        match self.socket_map.get_mut( socket ) {
            Some( plugin_list ) => Ok( load_plugins( &self.engine, plugin_list ) ),
            Option::None => Err( InvalidSocket( socket.clone() ) ),
        }
    }
}

fn load_plugins( engine: &Engine, plugin_list: &mut Vec<PluginTreeNode> ) -> Vec<LoaderError> {

    let ( new_list, errors ) = plugin_list
        .drain( .. )
        .map(| plugin_node | {
            match plugin_node {
                PluginTreeNode::LazyPlugin( plugin_data ) => {
                    load_plugin( engine, plugin_data )
                        .map( PluginTreeNode::ActivePlugin )
                }
                other_node => Ok( other_node ),
            }
        })
        .partition_result();

    *plugin_list = new_list ;
    errors

}