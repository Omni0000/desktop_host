use std::collections::HashMap ;
use std::sync::{ Arc, RwLock };
use pipe_trait::Pipe;
use wasmtime::Engine;
use wasmtime::component::Linker ;

use crate::utils::{ MapScanTrait, Merge };
use super::super::InterfaceId ;
use super::super::InterfaceCardinality ;
use super::super::{ RawInterfaceData, RawPluginData };
use super::plugin_tree::Socket ;
use super::plugin_instance::PluginInstance ;
use super::preload_plugin_tree::{ PluginPreloadError, PreloadResult };
use super::preload_plugin::preload_plugin ;
use super::plugin_context::PluginContext ;



#[derive( Debug )]
pub(super) enum SocketState {
    Unprocessed( RawInterfaceData, Vec<RawPluginData> ),
    Preloaded( Arc<RawInterfaceData>, Arc<Socket<RwLock<PluginInstance>>> ),
    Failed,
    Borrowed,
}
impl Default for SocketState { fn default() -> Self { Self::Borrowed }}
impl From<( RawInterfaceData, Vec<RawPluginData> )> for SocketState {
    fn from(( interface, plugins ): ( RawInterfaceData, Vec<RawPluginData> ) ) -> Self { Self::Unprocessed( interface, plugins )}
}

pub(super) fn preload_socket(
    mut socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    socket_id: InterfaceId
) -> PreloadResult<(
    Arc<RawInterfaceData>,
    Arc<Socket<RwLock<PluginInstance>>>,
)> {

    // NOTE: do not forget to add the entry back if it's already preloaded
    let socket_plugins = match socket_map.insert( socket_id, SocketState::Borrowed ) {
        Some( plugins ) => plugins,
        // REDUNDANT: all requested sockets should have been handled in discovery
        Option::None => return PreloadResult { socket_map, result: Err( PluginPreloadError::InvalidSocket( socket_id.clone() )), errors: Vec::with_capacity( 0 )},
    };

    match socket_plugins {
        SocketState::Borrowed => PreloadResult { socket_map, result: Err( PluginPreloadError::LoopDetected( socket_id )), errors: Vec::with_capacity( 0 ) },
        SocketState::Failed => PreloadResult { socket_map, result: Err( PluginPreloadError::AlreadyHandled ), errors: Vec::with_capacity( 0 ) },
        SocketState::Preloaded( interface, plugins ) => {
            let interface_arc = Arc::clone( &interface );
            let plugins_arc = Arc::clone( &plugins );
            // NOTE: readding entry since it was taken out to gain ownership
            socket_map.insert( socket_id, SocketState::Preloaded( interface, plugins ));
            PreloadResult { socket_map, result: Ok(( interface_arc, plugins_arc )), errors: Vec::with_capacity( 0 ) }
        },
        SocketState::Unprocessed( interface, plugins ) => {
            let PreloadResult { mut socket_map, result, errors } = preload_socket_unprocessed(
                socket_map, interface, plugins, engine, default_linker );
            match result {
                Ok(( interface, plugins )) => {
                    let interface = Arc::new( interface );
                    let plugins = Arc::new( wrap_plugin_tree_socket( plugins ));
                    socket_map.insert( socket_id, SocketState::Preloaded( Arc::clone( &interface ), Arc::clone( &plugins )));
                    PreloadResult { socket_map, result: Ok(( interface, plugins )), errors }
                },
                Err( err ) => {
                    socket_map.insert( socket_id, SocketState::Failed );
                    PreloadResult { socket_map, result: Err( err ), errors }
                }
            }
        }
    }

}

#[inline( always )] fn wrap_plugin_tree_socket( socket: Socket<PluginInstance> ) -> Socket<RwLock<PluginInstance>> {
    match socket {
        Socket::AtMostOne( Option::None ) => Socket::AtMostOne( Option::None ),
        Socket::AtMostOne( Some( plugin )) => Socket::AtMostOne( Some( RwLock::new( plugin ) )),
        Socket::ExactlyOne( plugin ) => Socket::ExactlyOne( RwLock::new( plugin )),
        Socket::AtLeastOne( plugins ) => Socket::AtLeastOne( plugins.into_iter().map( RwLock::new ).collect()),
        Socket::Any( plugins ) => Socket::Any( plugins.into_iter().map( RwLock::new ).collect()),
    }
}

#[inline] fn preload_socket_unprocessed(
    socket_map: HashMap<InterfaceId, SocketState>,
    interface: RawInterfaceData,
    plugins: Vec<RawPluginData>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
) -> PreloadResult<( RawInterfaceData, Socket<PluginInstance> )> {
    
    let cardinality = interface.get_cardinality();
    
    match cardinality {
        InterfaceCardinality::AtMostOne => preload_most_one( socket_map, engine, default_linker, plugins )
            .pipe(| PreloadResult { socket_map, result, errors } | match result {
                Ok( plugin_opt ) => PreloadResult { socket_map, result: Ok(( interface, Socket::AtMostOne( plugin_opt ))), errors },
                Err( err ) => PreloadResult { socket_map, result: Err( err ), errors },
            }),
        InterfaceCardinality::ExactlyOne => preload_exact_one( socket_map, engine, default_linker, plugins )
            .pipe(| PreloadResult { socket_map, result, errors } | match result {
                Ok( plugin ) => PreloadResult { socket_map, result: Ok(( interface, Socket::ExactlyOne( plugin ))), errors },
                Err( err ) => PreloadResult { socket_map, result: Err( err ), errors },
            }),
        InterfaceCardinality::AtLeastOne => preload_at_least_one( socket_map, engine, default_linker, plugins )
            .pipe(| PreloadResult { socket_map, result, errors } | match result {
                Ok( plugins ) => PreloadResult { socket_map, result: Ok(( interface, Socket::AtLeastOne( plugins ))), errors },
                Err( err ) => PreloadResult { socket_map, result: Err( err ), errors },
            }),
        InterfaceCardinality::Any => preload_any( socket_map, engine, default_linker, plugins )
            .pipe(|( socket_map, plugins, errors )| PreloadResult { socket_map, result: Ok(( interface, Socket::Any( plugins ))), errors }),
    }
}

#[inline] fn preload_most_one(
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    mut plugins: Vec<RawPluginData>,
) -> PreloadResult<Option<PluginInstance>> {
    match plugins.pop() {
        Option::None => PreloadResult { socket_map, result: Ok( None ), errors: Vec::with_capacity( 0 ) },
        Some( plugin ) => match plugins.pop() {
            Option::None => match preload_plugin( socket_map, engine, default_linker, plugin ) {
                PreloadResult { socket_map, result: Ok( plugin ), errors } => PreloadResult { socket_map, result: Ok( Some( plugin )), errors },
                PreloadResult { socket_map, result: Err( err ), errors } => PreloadResult { socket_map, result: Ok( None ), errors: errors.merge( err ) },
            },
            Some( _ ) => PreloadResult {
                socket_map,
                result: Err( PluginPreloadError::FailedCardinalityRequirements( InterfaceCardinality::AtMostOne, plugins.len() +2 )),
                errors: Vec::with_capacity( 0 )
            },
        }
    }
}

#[inline] fn preload_exact_one(
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    mut plugins: Vec<RawPluginData>,
) -> PreloadResult<PluginInstance> {

    match plugins.pop() {
        Option::None => PreloadResult {
            socket_map,
            result: Err( PluginPreloadError::FailedCardinalityRequirements( InterfaceCardinality::ExactlyOne, 0 )),
            errors: Vec::with_capacity( 0 )
        },
        Some( plugin ) => match plugins.pop() {
            Option::None => preload_plugin( socket_map, engine, default_linker, plugin ),
            Some( _ ) => PreloadResult {
                socket_map,
                result: Err( PluginPreloadError::FailedCardinalityRequirements( InterfaceCardinality::ExactlyOne, plugins.len() +2 )),
                errors: Vec::with_capacity( 0 )
            },
        }
    }

}

#[inline] fn preload_at_least_one(
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    plugins: Vec<RawPluginData>,
) -> PreloadResult<Vec<PluginInstance>> {

    if plugins.len() < 1 { return PreloadResult {
        socket_map,
        result: Err( PluginPreloadError::FailedCardinalityRequirements( InterfaceCardinality::AtLeastOne, 0 )),
        errors: Vec::with_capacity( 0 ),
    }};

    let ( socket_map, plugins, errors ) = preload_any( socket_map, engine, default_linker, plugins );

    if plugins.len() < 1 { return PreloadResult {
        socket_map,
        result: Err( PluginPreloadError::FailedCardinalityRequirements( InterfaceCardinality::AtLeastOne, 0 )),
        errors,
    }};

    PreloadResult { socket_map, result: Ok( plugins ), errors }

}

#[inline] fn preload_any(
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    plugins: Vec<RawPluginData>,
) -> (
    HashMap<InterfaceId, SocketState>,
    Vec<PluginInstance>,
    Vec<PluginPreloadError>,
) {

    let (( plugins, errors ), socket_map ) = plugins.into_iter().map_scan(
        socket_map,
        | plugin, socket_map | match preload_plugin( socket_map, engine, default_linker, plugin ) {
            PreloadResult { socket_map, result: Ok( plugin ), errors } => (( Some( plugin ), errors ), socket_map ),
            PreloadResult { socket_map, result: Err( err ), errors } => (( None, errors.merge( err )), socket_map )
        }
    ).unzip_get_state::<Vec<_>, Vec<_>>();

    let plugins = plugins.into_iter().flatten().collect::<Vec<_>>();
    let errors = errors.into_iter().flatten().collect::<Vec<_>>();

    ( socket_map, plugins, errors )

}