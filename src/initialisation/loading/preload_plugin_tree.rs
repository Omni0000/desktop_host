use std::sync::{ Arc, RwLock };
use std::collections::HashMap ;
use thiserror::Error ;
use wasmtime::Engine;
use wasmtime::component::Linker ;

use crate::InterfaceId ; 
use super::{ RawInterfaceData, RawPluginData, InterfaceCardinality, InterfaceManifestReadError, PluginManifestReadError };
use super::{ Socket, PluginInstance, PluginContext, preload_socket, SocketState };



#[derive( Error, Debug )]
pub enum PreloadError {
    
    #[error( "Invalid socket: {0}" )]
    InvalidSocket( InterfaceId ),
    
    #[error( "Loop detected loading: '{0}'" )]
    LoopDetected( InterfaceId ),
    
    #[error( "Failed to meet cardinality requirements: {0}, found {1}" )]
    FailedCardinalityRequirements( InterfaceCardinality, usize ),
    
    #[error( "Corrupted plugin manifest: {0}" )]
    CorruptedPluginManifest( PluginManifestReadError ),
    
    #[error( "Corrupted interface manifest: {0}" )]
    CorruptedInterfaceManifest( InterfaceManifestReadError ),

    #[error( "Failed to load component: {0}" )]
    FailedToLoadComponent( wasmtime::Error ),

    #[error( "Failed to link root interface: {0}" )]
    FailedToLinkRootInterface( wasmtime::Error ),

    #[error( "Failed to link function '{0}': {1}" )]
    FailedToLink( String, wasmtime::Error ),

    #[error( "Handled failure" )]
    AlreadyHandled,

}

#[derive(Debug)]
pub(super) struct PreloadResult<T> {
    pub socket_map: HashMap<InterfaceId, SocketState>,
    pub result: Result<T, PreloadError>,
    pub errors: Vec<PreloadError>,
}

#[inline] pub(super) fn preload_plugin_tree(
    socket_map: HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    root: InterfaceId,
) -> Result<(
    Arc<RawInterfaceData>,
    Arc<Socket<RwLock<PluginInstance>>>,
    Vec<PreloadError>,
), (
    PreloadError,
    Vec<PreloadError>,
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
