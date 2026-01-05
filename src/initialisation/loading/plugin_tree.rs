use std::sync::{ Arc, RwLock };
use std::collections::HashMap ;
use wasmtime::component::Linker ;
use wasmtime::Engine ;

use crate::InterfaceId ;
use super::{ RawInterfaceData, RawPluginData };
use super::{ preload_plugin_tree, Socket, PluginInstance, PluginContext, PreloadError };



pub struct PluginTree {
    pub(super) root_socket: Arc<Socket<RwLock<PluginInstance>>>,
    pub(super) _root_interface: Arc<RawInterfaceData>,
}

impl PluginTree {
    pub fn new(
        root: InterfaceId,
        socket_map: HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )>,
        engine: Engine,
        linker: &Linker<PluginContext>,
    ) -> (
        Result<Self, PreloadError>,
        Vec<PreloadError>
    ) {

        match preload_plugin_tree( socket_map, &engine, linker, root ) {
            Ok(( _root_interface, root_socket, errors )) => ( Ok( Self { _root_interface, root_socket }), errors ),
            Err(( err, errors )) => ( Err( err ), errors ),
        }

    }
}

