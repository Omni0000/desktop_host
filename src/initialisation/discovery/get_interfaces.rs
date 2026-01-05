use std::collections::{ HashSet, HashMap };
use itertools::Itertools ;

use crate::initialisation::InterfaceId ;
use super::{ DiscoveryError, RawPluginData, RawInterfaceData };



pub(super) fn try_into_socket_map( plugins: Vec<RawPluginData> ) -> ( HashMap<InterfaceId, Vec<RawPluginData>>, Vec<DiscoveryError> ) {
    let ( plugins, errors ) = plugins.into_iter()
        .map(| mut plugin | Ok((
            plugin.get_plug()
                .map_err(| err | DiscoveryError::FailedToReadManifest( plugin.id().clone(), err ))?,
            plugin
        )))
        .partition_result::<Vec<_>, Vec<_>, _, _>();
    let plugins = plugins.into_iter().fold( HashMap::new(), | mut acc, ( plug_id, plugin )| {
        acc.entry( plug_id )
            .or_insert_with( Vec::new )
            .push( plugin );
        acc
    });
    ( plugins, errors )
}

pub(super) fn try_get_all_interfaces_from_cache( interfaces_ids: HashSet<InterfaceId> ) -> (
    Vec<RawInterfaceData>,
    Vec<( InterfaceId, DiscoveryError )>
) {
    interfaces_ids.into_iter()
        .map(| id | RawInterfaceData::new( id ).map_err(| err | ( id, err )))
        .partition_result::<Vec<_>, Vec<_>, _, _ >()
}

pub(super) fn try_download_all_interfaces( interface_ids: Vec<InterfaceId> ) -> (
    Vec<RawInterfaceData>,
    Vec<DiscoveryError>,
) {
    interface_ids.iter()
        .map(| id | unimplemented!( "Interface downloading is not yet supported, attempted to download: {id}" ))
        .partition_result()
}