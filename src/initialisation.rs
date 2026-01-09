use std::path::PathBuf ;
use thiserror::Error ;
use wasmtime::Engine ;

mod discovery ;
mod loading ;
mod types ;

pub use types::{ PluginId, InterfaceId };
pub use discovery::{ RawPluginData as PluginData };
pub use loading::{ PluginTree, PluginContext };
use discovery::{ RawPluginData, RawInterfaceData, InterfaceCardinality,
    FunctionData, FunctionReturnType, InterfaceManifestReadError, PluginManifestReadError, InterfaceParseError };



#[derive( Error, Debug )]
pub enum UnrecoverableStartupError {
    #[error( "Discovery Failure: {0}" )] DiscoveryError( #[from] discovery::DiscoveryFailure ),
    #[error( "Plugin Preload Error: {0}" )] PluginPreloadError( #[from] loading::PreloadError ),
}

pub fn initialise_plugin_tree( source: &PathBuf, root_interface_id: &InterfaceId ) -> Result<PluginTree, UnrecoverableStartupError> {

    let ( socket_map, plugin_discovery_errors ) = discovery::discover_all( source )?;
    plugin_discovery_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    let engine = Engine::default();
    
    let ( linker, linker_errors ) = crate::exports::exports( &engine );
    linker_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    let ( plugin_tree, preload_errors ) = PluginTree::new( root_interface_id.clone(), socket_map, engine, &linker );
    preload_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    Ok( plugin_tree? )

}
