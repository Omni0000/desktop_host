
use itertools::Itertools ;

use std::collections::HashMap ;
use thiserror::Error ;
use crate::startup::plugin_deserialiser::{ Plugin, InterfaceId };



#[derive( Error, Debug )]
pub enum PluginPreprocessorError {
    #[error("capnp: {0}")] Capnp( #[from] capnp::Error ),
    #[error("Utf8Error")] Utf8Error( #[from] std::str::Utf8Error ),
}

pub(in super::super) type SocketMap<T> = HashMap<InterfaceId, Vec<T>> ;

pub fn build_socket_map( plugins: Vec<Plugin> ) -> ( SocketMap<Plugin>, Vec<PluginPreprocessorError> ) {
    
    let ( parsed, errors ): ( Vec<_>, Vec<_> ) = plugins
        .into_iter()
        .map(| plugin | {
            let plug_id = plugin.manifest().get_plug()?.get_id()?.to_string()?;
            Ok(( plug_id, plugin ))
        })
        .partition_result();
    
    (
        parsed
            .into_iter()
            .fold( HashMap::new(), | mut acc, ( k, v )| {
                acc.entry( k ).or_insert_with( Vec::new ).push( v );
                acc
            }),
        errors
    )

}
