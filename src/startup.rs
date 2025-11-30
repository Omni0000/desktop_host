
use thiserror::Error ;

mod plugin_discovery ;
mod plugin_deserialiser ;
mod plugin_preprocessor ;
mod plugin_loader ;

pub use plugin_deserialiser::{ Plugin, InterfaceId };
use plugin_preprocessor::build_socket_map ;
use plugin_loader::{ LivePluginTree, build_live_plugin_tree };



#[derive( Error, Debug )]
pub enum StartupError {
    #[error( "Plugin Cache Error: {0}" )] PluginCacheError( #[from] plugin_discovery::PluginCacheError ),
    #[error( "Deserialisation Error: {0}" )] DeserialisationError( #[from] plugin_deserialiser::DecoderError ),
    #[error( "Preprocessor Error: {0}" )] ParserError( #[from] plugin_preprocessor::PluginPreprocessorError ),
}

pub fn startup() -> Result<LivePluginTree, StartupError> {

    let ( plugin_data, plugin_discovery_errors ) = plugin_discovery::get_plugins()?;
    plugin_discovery_errors.iter().for_each(| err | produce_warning( err ));

    let ( plugins, plugin_deserialisation_errors ) = plugin_deserialiser::parse_plugins( plugin_data );
    plugin_deserialisation_errors.iter().for_each(| err | produce_warning( err ));

    let ( socket_map, plugin_preprocessing_errors ) = build_socket_map( plugins );
    plugin_preprocessing_errors.iter().for_each(| err | produce_warning( err ));

    Ok( build_live_plugin_tree( socket_map ))

}

fn produce_warning<T: std::fmt::Display>( message: T ) {
    println!( "Warning: {}", message );
}