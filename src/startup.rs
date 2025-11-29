
use thiserror::Error ;

mod plugin_discovery ;
mod plugin_deserialiser ;
mod plugin_preprocessor ;
mod plugin_loader ;

pub use plugin_deserialiser::{ Plugin, PluginId, InterfaceId };
use plugin_preprocessor::build_socket_map ;



#[derive( Error, Debug )]
pub enum StartupError {
    #[error("PluginCacheError: {0}")] PluginCacheError( #[from] plugin_discovery::PluginCacheError ),
    #[error("DeserialisationError: {0}")] DeserialisationError( #[from] plugin_deserialiser::DecoderError ),
    #[error("PreprocessorError: {0}")] ParserError( #[from] plugin_preprocessor::PluginParserError ),
}

pub fn startup() -> Result<(),StartupError> {

    let ( plugin_data, plugin_discovery_errors ) = plugin_discovery::get_plugins()?.deconstruct();
    plugin_discovery_errors.iter().for_each(| err | produce_warning( err ));

    let ( plugins, plugin_deserialisation_errors ) = plugin_deserialiser::parse_plugins( plugin_data ).deconstruct();
    plugin_deserialisation_errors.iter().for_each(| err | produce_warning( err ));

    let _socket_map = build_socket_map( plugins )?;

    Ok(())

}

fn produce_warning<T: std::fmt::Display>( message: T ) {
    println!( "Warning: {message}" );
}