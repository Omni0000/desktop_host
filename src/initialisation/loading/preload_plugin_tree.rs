use std::sync::{ Arc, RwLock };
use std::collections::HashMap ;
use thiserror::Error ;
use wasmtime::Engine;
use wasmtime::component::Linker ;

use super::super::discovery::{ RawInterfaceData, RawPluginData, ManifestReadError };
use super::super::InterfaceId ; 
use super::super::InterfaceCardinality ;
use super::plugin_tree::Socket ;
use super::plugin_instance::PluginInstance ;
use super::preload_socket::{ preload_socket, SocketState };
use super::plugin_context::PluginContext ;



#[derive( Error, Debug )]
pub enum PluginPreloadError {
    
    #[error( "Invalid socket: {0}" )]
    InvalidSocket( InterfaceId ),
    
    #[error( "Loop detected loading: '{0}'" )]
    LoopDetected( InterfaceId ),
    
    #[error( "Failed to meet cardinality requirements: {0}, found {1}" )]
    FailedCardinalityRequirements( InterfaceCardinality, usize ),
    
    #[error( "Corrupted plugin manifest: {0}" )]
    CorruptedPluginManifest( ManifestReadError ),
    
    #[error( "Failed to load component: {0}" )]
    FailedToLoadComponent( wasmtime::Error ),

    #[error( "Failed to link root interface: {0}" )]
    FailedToLinkRootInterface( wasmtime::Error ),

    #[error( "Failed to link function '{0}': {1}" )]
    FailedToLinkFunction( String, wasmtime::Error ),

    #[error( "Handled failure" )]
    AlreadyHandled,

}

#[derive(Debug)]
pub(super) struct PreloadResult<T> {
    pub socket_map: HashMap<InterfaceId, SocketState>,
    pub result: Result<T, PluginPreloadError>,
    pub errors: Vec<PluginPreloadError>,
}

#[inline] pub(super) fn preload_plugin_tree(
    socket_map: HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    root: InterfaceId,
) -> Result<(
    Arc<RawInterfaceData>,
    Arc<Socket<RwLock<PluginInstance>>>,
    Vec<PluginPreloadError>,
), (
    PluginPreloadError,
    Vec<PluginPreloadError>,
)> {

    match preload_socket(
        wrap_unprocessed( socket_map ),
        engine,
        default_linker,
        root,
    ) {
        PreloadResult { socket_map: _, result: Ok(( interface, socket )), errors } => Ok(( interface, socket, errors )),
        PreloadResult { socket_map: _, result: Err( err ), errors } => Err(( err, errors ))
    }

}

#[inline( always )] fn wrap_unprocessed(
    socket_map: HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )>,
) -> HashMap<InterfaceId, SocketState> {
    socket_map.into_iter()
        .map(|( socket_id, ( interface, plugins ))| ( socket_id, SocketState::Unprocessed( interface, plugins )))
        .collect()
}
