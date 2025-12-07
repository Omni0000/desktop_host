
mod plugin_discovery ;
mod plugin_deserialiser ;
mod plugin_preprocessor ;
mod plugin_loader ;

pub use plugin_deserialiser::{ Plugin, InterfaceId };
pub use plugin_loader::{ LivePluginTree, FunctionDispatchInstruction };
pub use plugin_loader::{ WasmMemorySegment, WasmSendContext, WasmMemSegPtr, WasmMemSegSize, RawMemorySegment, MemoryReadError, MemorySendError };
use plugin_preprocessor::build_socket_map ;

use wasmtime::Engine ;



pub fn startup() -> Result<LivePluginTree, plugin_discovery::PluginCacheError> {

    let ( plugin_data, plugin_discovery_errors ) = plugin_discovery::get_plugins()?;
    plugin_discovery_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    let ( plugins, plugin_deserialisation_errors ) = plugin_deserialiser::parse_plugins( plugin_data );
    plugin_deserialisation_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    let ( socket_map, plugin_preprocessing_errors ) = build_socket_map( plugins );
    plugin_preprocessing_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    let engine = Engine::default();
    let ( linker, linker_errors ) = crate::exports::exports( &engine );
    let live_plugin_tree = LivePluginTree::new( engine, socket_map, linker );
    linker_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

    Ok( live_plugin_tree )

}
