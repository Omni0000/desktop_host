use std::collections::HashMap ;
use thiserror::Error ;

use crate::utils::Merge ;
use crate::initialisation::{ PluginId, InterfaceId };
use super::{ RawPluginData, RawInterfaceData, PluginManifestReadError, InterfaceParseError,
    try_get_all_cached_plugins, try_download_plugins, try_get_used_interfaces,
    try_into_socket_map, try_get_all_interfaces_from_cache, try_download_all_interfaces
};



#[derive( Error, Debug )]
pub enum DiscoveryError {

    #[error( "Plugin not in cache: {0}" )]
    PluginNotInCache( PluginId ),

    #[error( "Download failed: {0}" )]
    DownloadFailed( PluginId ),

    #[error( "Download failed: {0}" )]
    DownloadFailedInterface( InterfaceId ),
    
    #[error( "Interface not in cache: {0}" )]
    InterfaceNotInCache( InterfaceId ),

    #[error( "Unused interface in cache: {0}" )]
    UnusedInterface( InterfaceId ),

    #[error( "Missing required interface: {0}" )]
    MissingRequiredInterface( InterfaceId ),

    #[error( "Failed to parse interface {0}: {1}" )]
    FailedToParseInterface( InterfaceId, InterfaceParseError ),

    #[error( "Failed to read from plugin manifest of '{0}': {1}" )]
    FailedToReadPluginManifest( PluginId, PluginManifestReadError ),

}

#[derive( Error, Debug )]
pub enum DiscoveryFailure {

}

pub fn discover_all() -> Result<(
    HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )>,
    Vec<DiscoveryError>,
), DiscoveryFailure> {

    let cached_plugin_ids = read_cache_header()?;

    let ( cached_plugins, missing_plugins ) = try_get_all_cached_plugins( cached_plugin_ids );
    let ( missing_plugin_ids, errors ) = missing_plugins.into_iter().unzip::<_, _, _, Vec<_>>();

    let ( downloaded_plugins, plugin_download_errors ) = try_download_plugins( missing_plugin_ids );
    let errors = errors.merge_all( plugin_download_errors );

    let ( plugins, used_interface_ids, manifest_errors ) = try_get_used_interfaces( cached_plugins.into_iter().chain( downloaded_plugins.into_iter() ));
    let errors = errors.merge_all( manifest_errors );

    let ( plugins, manifest_errors ) = try_into_socket_map( plugins );
    let errors = errors.merge_all( manifest_errors );

    let ( cached_interfaces, missing_interfaces ) = try_get_all_interfaces_from_cache( used_interface_ids );
    let ( missing_interface_ids, missing_interface_errors ) = missing_interfaces.into_iter().unzip::<_,_,_,Vec<_>>();
    let errors = errors.merge_all( missing_interface_errors );

    let ( downloaded_interfaces, interface_download_errors ) = try_download_all_interfaces( missing_interface_ids );
    let errors = errors.merge_all( interface_download_errors );

    let socket_map = build_socket_map( cached_interfaces.into_iter().chain( downloaded_interfaces.into_iter() ), plugins );

    Ok(( socket_map, errors ))

}

fn read_cache_header() -> Result<Vec<PluginId>, DiscoveryFailure> {
    Ok( vec![
        "foo".to_string(),
        "bar".to_string(),
    ])
}


fn build_socket_map(
    interfaces: impl Iterator<Item = RawInterfaceData>,
    mut plugins: HashMap<InterfaceId, Vec<RawPluginData>>
) -> HashMap<InterfaceId, ( RawInterfaceData, Vec<RawPluginData> )> {
    interfaces
        .map(| interface | {
            let plugins_list = plugins.remove( interface.id() ).unwrap_or( Vec::new() );
            ( interface.id().clone(), ( interface, plugins_list ))
        })
        .collect()
}
