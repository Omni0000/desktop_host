
use std::collections::HashSet ;
use itertools::Itertools ;

use crate::utils::Merge ;
use crate::initialisation::{ PluginId, InterfaceId };
use super::{ DiscoveryError, RawPluginData };



pub(super) fn try_get_all_cached_plugins( plugin_ids: Vec<PluginId> ) -> (
    Vec<RawPluginData>,
    Vec<( PluginId, DiscoveryError )>
) {
    plugin_ids.into_iter()
        .map(| id | RawPluginData::new( &id ).map_err(| err | ( id, err )))
        .partition_result::<Vec<_>, Vec<_>, _, _ >()
}

pub(super) fn try_download_plugins( plugin_ids: Vec<PluginId> ) -> (
    Vec<RawPluginData>,
    Vec<DiscoveryError>,
) {
    plugin_ids.iter()
        .map(| id | unimplemented!( "Plugin downloading is not yet supported, attempted to download: {id}" ))
        .partition_result()
}

pub(super) fn try_get_used_interfaces<'a>( plugins: impl Iterator<Item = RawPluginData> ) -> (
    Vec<RawPluginData>,
    HashSet<InterfaceId>,
    Vec<DiscoveryError>
) {
    let ( successful, errors ) = plugins.into_iter()
        .map(| mut plugin| get_used_interfaces( &mut plugin ).map(|interfaces| (plugin, interfaces)))
        .partition_result::<Vec<_>, Vec<_>, _, _>();
    let ( successful_plugins, interfaces ) = successful.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
    
    let interfaces = interfaces.into_iter().flatten().collect::<HashSet<_>>();

    ( successful_plugins, interfaces, errors )
}

fn get_used_interfaces( plugin: &mut RawPluginData ) -> Result<Vec<InterfaceId>, DiscoveryError> {

    let plug = match plugin.get_plug() {
        Ok( data ) => data,
        Err( err ) => return Err( DiscoveryError::FailedToReadPluginManifest( plugin.id().clone(), err )),
    };

    let sockets = match plugin.get_sockets() {
        Ok( data ) => data,
        Err( err ) => return Err( DiscoveryError::FailedToReadPluginManifest( plugin.id().clone(), err )),
    };
    
    Ok( sockets.merge( plug ))

}
