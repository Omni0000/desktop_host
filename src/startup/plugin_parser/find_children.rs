
use std::collections::HashMap ;
use thiserror::Error ;
use crate::startup::plugin_decoder::Plugin ;
use crate::startup::interface_decoder::InterfaceId ;



#[derive( Error, Debug )]
pub enum PluginParserError {
    #[error("capnp: {0}")] Capnp( #[from] capnp::Error ),
    #[error("Utf8Error")] Utf8Error( #[from] std::str::Utf8Error ),
}

pub fn find_children( plugins: Vec<Plugin> ) -> Result<HashMap<InterfaceId,Vec<Plugin>>, PluginParserError> {
    
    Ok( plugins
        .into_iter()
        .map(| plugin | {
            let plug_id = plugin.manifest().get_plug()?.get_id()?.to_string()?;
            Ok(( plug_id, plugin ))
        })
        .collect::<Result<Vec<_>, PluginParserError>>()?
        .into_iter()
        .fold( HashMap::new(), | mut acc, ( k, v )| {
            acc.entry( k ).or_insert_with( Vec::new ).push( v );
            acc
        })
    )

}
