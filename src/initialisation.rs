use std::path::Path ;
use thiserror::Error ;
use wasmtime::Engine ;
use pipe_trait::Pipe ;

mod discovery ;
mod loading ;
mod types ;

pub use types::{ PluginId, InterfaceId };
pub use discovery::{ RawPluginData as PluginData, InterfaceCardinality, DiscoveryError, DiscoveryFailure };
pub use loading::{ PluginTree, PluginContext, Socket, PreloadError };
use discovery::{ RawPluginData, RawInterfaceData, RawSocketMap, FunctionData, FunctionReturnType,
    InterfaceManifestReadError, PluginManifestReadError, InterfaceParseError, discover_all };

use crate::utils::{ PartialResult, deconstruct_partial_result };



#[derive( Error, Debug )]
pub enum UnrecoverableStartupError {
    #[error( "Discovery Failure: {0}" )] DiscoveryError( #[from] DiscoveryFailure ),
    #[error( "Plugin Error: {0}" )] PreloadError( #[from] PreloadError ),
}

#[derive( Error, Debug )]
pub enum RecoverableStartupError {
    #[error( "Discovery Error: {0}" )] DiscoveryError( #[from] DiscoveryError ),
    #[error( "Linker Error: {0}" )] LinkerError( wasmtime::Error ),
    #[error( "Preload Error:" )] PreloadError( #[from] PreloadError ),
}

pub fn initialise_plugin_tree( source: &Path, root_interface_id: &InterfaceId ) -> PartialResult<PluginTree, UnrecoverableStartupError, RecoverableStartupError> {

    let ( socket_map, discovery_errors ) = discover_all( source, root_interface_id ).map_err(| err | ( err.into(), vec![] ))?;

    let engine = Engine::default();
    let ( linker, linker_errors ) = crate::exports::exports( &engine );

    let ( preload_result, preload_errors ) = PluginTree::new( *root_interface_id, socket_map, engine, &linker )
        .pipe( deconstruct_partial_result );

    let errors = discovery_errors.into_iter().map( Into::into )
        .chain( linker_errors.into_iter().map( RecoverableStartupError::LinkerError ))
        .chain( preload_errors.into_iter().map( Into::into ))
        .collect();

    match preload_result {
        Ok( plugin_tree ) => Ok(( plugin_tree, errors )),
        Err( preload_failure ) => Err(( preload_failure.into(), errors )),
    }

}
