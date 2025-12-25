use std::sync::{ Arc, RwLock };
use std::collections::HashMap ;
use wasmtime::component::Linker ;
use wasmtime::Engine ;


use crate::initialisation::loading::PluginContext;

use super::super::InterfaceId ;
use super::super::discovery::{ RawInterfaceData, RawPluginData };
use super::preload_plugin_tree::{ preload_plugin_tree, PluginPreloadError };
use super::plugin_instance::PluginInstance ;

pub struct PluginTree {
    pub(super) root_socket: Arc<Socket<RwLock<PluginInstance>>>,
    pub(super) root_interface: Arc<RawInterfaceData>,
}

impl PluginTree {
    pub fn new(
        root: InterfaceId,
        socket_map: HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )>,
        engine: Engine,
        linker: &Linker<PluginContext>,
    ) -> (
        Result<Self, PluginPreloadError>,
        Vec<PluginPreloadError>
    ) {

        match preload_plugin_tree( socket_map, &engine, linker, root ) {
            Ok(( root_interface, root_socket, errors )) => ( Ok( Self { root_interface, root_socket }), errors ),
            Err(( err, errors )) => ( Err( err ), errors ),
        }

    }
}

#[derive( Debug )]
pub enum Socket<T> {
    AtMostOne( Option<T> ),
    ExactlyOne( T ),
    AtLeastOne( Vec<T> ),
    Any( Vec<T> ),
}