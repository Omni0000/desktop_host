use std::collections::HashMap ;
use std::sync::{ Arc, RwLock };
use wasmtime::{ Engine, Store };
use wasmtime::component::{ Linker, Component };

use crate::utils::Merge ;
use crate::initialisation::InterfaceId ;
use super::super::discovery::{ RawPluginData, RawInterfaceData };
use super::plugin_instance::PluginInstance ;
use super::preload_plugin_tree::{ PluginPreloadError, PreloadResult };
use super::preload_socket::{ preload_socket, SocketState };
use super::plugin_context::PluginContext ;
use super::Socket ;



#[inline] pub fn preload_plugin(
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    mut plugin: RawPluginData,
) -> PreloadResult< PluginInstance > {
    
    let socket_ids = match plugin.get_sockets() {
        Ok( ids ) => ids,
        Err( err ) => return PreloadResult { socket_map, result: Err( PluginPreloadError::CorruptedPluginManifest( err ) ), errors: Vec::with_capacity( 0 ) }
    };

    let ( socket_map, sockets, errors ) = match preload_child_sockets( &socket_ids, socket_map, engine, default_linker ) {
        PreloadResult { socket_map, result: Ok( sockets ), errors } => ( socket_map, sockets, errors ),
        PreloadResult { socket_map, result: Err( err ), errors } => return PreloadResult { socket_map, result: Err( err ), errors },
    };

    let linker = match sockets.into_iter().try_fold(
        default_linker.clone(),
        | linker, ( interface, socket )| link_socket_functions( linker, interface, socket ),
    ) {
        Ok( linker ) => linker,
        Err( err ) => return PreloadResult { socket_map, result: Err( err ), errors },
    };

    let component = match Component::from_file( linker.engine(), plugin.wasm_path() ) {
        Ok( component ) => component,
        Err( err ) => return PreloadResult { socket_map, result: Err( PluginPreloadError::FailedToLoadComponent( err )), errors },
    };

    let plugin_id = plugin.id().clone();
    let host_data = PluginContext::new( plugin );
    let mut store = Store::new( engine, host_data );
    let instance = match linker.instantiate( &mut store, &component ) {
        Ok( instanace_pre ) => instanace_pre,
        Err( err ) => return PreloadResult { socket_map, result: Err( PluginPreloadError::FailedToLoadComponent( err )), errors },
    };

    let lazy_plugin = PluginInstance {
        _id: plugin_id,
        _component: component,
        store,
        instance,
    };

    PreloadResult { socket_map, result: Ok( lazy_plugin ), errors }

}

#[inline] fn preload_child_sockets(
    socket_ids: &Vec<InterfaceId>,
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
) -> PreloadResult< Vec<(
    Arc<RawInterfaceData>,
    Arc<Socket<RwLock<PluginInstance>>>,
)> > {

    match socket_ids.iter().try_fold(
        ( socket_map, Vec::<( Arc<RawInterfaceData>, Arc<Socket<RwLock<PluginInstance>>> )>::new(), Vec::<PluginPreloadError>::new() ),
        |( socket_map, sockets, errors ), socket_id | match preload_socket( socket_map, engine, default_linker, *socket_id ) {
            PreloadResult { socket_map, result: Ok( socket ), errors: new_errors } =>
                Ok(( socket_map, sockets.merge(( socket.0, socket.1 )), errors.merge_all( new_errors ) )),
            PreloadResult { socket_map, result: Err( err ), errors: new_errors } =>
                Err(( socket_map, err, errors.merge_all( new_errors ))),
        }
    ) {
        Ok(( socket_map, sockets, errors )) => PreloadResult { socket_map, result: Ok( sockets ), errors },
        Err(( socket_map, err, errors )) => PreloadResult { socket_map, result: Err( err ), errors }
    }

}

#[inline] fn link_socket_functions(
    mut linker: Linker<PluginContext>,
    interface: Arc<RawInterfaceData>,
    socket: Arc<Socket<RwLock<PluginInstance>>>,
) -> Result<Linker<PluginContext>, PluginPreloadError> {

    let package = interface.get_package().clone();
    let interface_ident = format!( "{}/root", package );
    let functions = interface.get_function_names();

    let mut root = linker.root();
    let mut instance = root.instance( &interface_ident )
        .map_err(| err | PluginPreloadError::FailedToLinkRootInterface( err ))?;

    functions.into_iter().try_for_each(| function | -> Result<(), PluginPreloadError> {
        let function_clone = function.to_string();
        let interface_ident_clone = interface_ident.clone();
        let interface_arc_clone = Arc::clone( &interface );
        let socket_arc_clone = Arc::clone( &socket );
        instance
            .func_new( &function, move | _ctx, _ty, args, results | {
                results[0] = socket_arc_clone.dispatch_into_val( &interface_ident_clone, &interface_arc_clone, &function_clone, args );
                Ok(())
            })
            .map_err(| err | PluginPreloadError::FailedToLinkFunction( function.to_string(), err ))
    })?;

    Ok( linker )

}
