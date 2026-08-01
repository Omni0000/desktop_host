use std::future::Future ;
use wasmtime::{ Config, Engine, Store };
use wasmtime::component::{ Component, FutureReader, Linker, ResourceTable, StreamReader, Val };

use super::{
	AsyncRequest,
	AsyncRuntimeError,
	AttachedDriver,
	DriverState,
	PluginInstanceAsync,
	PluginState,
	PluginInstanceAsyncInner,
	RequestQueues,
	ensure_supported_value,
};
use crate::{ DispatchError, Function, FunctionKind, PluginContext, ReturnKind };

struct Context { table: ResourceTable }

impl PluginContext for Context {
	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.table }
}

#[test]
fn async_requests_are_fifo_per_caller_and_round_robin_between_callers() {
	fn request( caller: u64, name: &str ) -> AsyncRequest {
		let ( response, _ ) = futures::channel::oneshot::channel();
		AsyncRequest {
			caller,
			package_name: String::new(),
			interface_name: String::new(),
			function_name: name.to_string(),
			function: Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
			data: Vec::new(),
			response,
		}
	}

	let mut queues = RequestQueues::default();
	queues.push( request( 1, "a1" ));
	queues.push( request( 1, "a2" ));
	queues.push( request( 2, "b1" ));
	queues.push( request( 2, "b2" ));
	let order = std::iter::from_fn(|| queues.pop().map(| request | request.function_name ))
		.collect::<Vec<_>>();
	assert_eq!( order, [ "a1", "b1", "a2", "b2" ]);
}

#[test]
fn dropped_response_guard_reports_the_cancellation_error() {
	let ( response, result ) = futures::channel::oneshot::channel();
	drop( super::ResponseGuard::new( response ));
	let Ok( Err( DispatchError::RuntimeException( error ))) = futures::executor::block_on( result ) else {
		panic!( "dropping the response guard did not return a runtime exception" );
	};
	assert_eq!( error.downcast_ref(), Some( &super::AsyncRuntimeError::CallCancelled ));
}

#[test]
fn failed_driver_rejects_later_calls_with_the_exact_error() {
	let expected = AsyncRuntimeError::StoreFailed( "expected".to_string() );
	let ( sender, _receiver ) = futures::channel::mpsc::unbounded();
	let inner = std::sync::Arc::new( PluginInstanceAsyncInner {
		sender,
		driver: std::sync::Mutex::new( DriverState::Failed( expected.clone() )),
		interface_remaps: std::collections::HashMap::new(),
		export_asyncness: std::collections::HashMap::new(),
		session_locks: std::sync::Arc::new( Vec::new() ),
	});
	let ( response, _result ) = futures::channel::oneshot::channel();
	let result = inner.enqueue(
		&crate::dispatch_session::SessionShared::new(),
		request( 0, "call", response ),
	);
	let Err( error ) = result else { panic!( "a failed driver accepted a later call" )};
	let DispatchError::RuntimeException( error ) = error else {
		panic!( "failed driver did not return a runtime exception" );
	};
	assert_eq!( error.downcast_ref(), Some( &expected ));
}

#[test]
fn stopped_driver_rejects_the_request_with_the_exact_error() {
	let ( sender, receiver ) = futures::channel::mpsc::unbounded();
	drop( receiver );
	let inner = std::sync::Arc::new( PluginInstanceAsyncInner {
		sender,
		driver: std::sync::Mutex::new( DriverState::Idle( Box::pin( futures::future::pending() ))),
		interface_remaps: std::collections::HashMap::new(),
		export_asyncness: std::collections::HashMap::new(),
		session_locks: std::sync::Arc::new( Vec::new() ),
	});
	let ( response, _result ) = futures::channel::oneshot::channel();
	let result = inner.enqueue(
		&crate::dispatch_session::SessionShared::new(),
		request( 0, "call", response ),
	);
	let Err( error ) = result else { panic!( "a stopped driver accepted the request" )};
	let DispatchError::RuntimeException( error ) = error else {
		panic!( "stopped driver did not return a runtime exception" );
	};
	assert_eq!( error.downcast_ref(), Some( &AsyncRuntimeError::DriverStopped ));
}

#[test]
fn async_dispatch_reports_exact_session_errors() {
	let ( sender, receiver ) = futures::channel::mpsc::unbounded();
	let plugin: PluginInstanceAsync<Context> = PluginInstanceAsync { kind: super::PluginInstanceAsyncKind::Async(
		std::sync::Arc::new( PluginInstanceAsyncInner {
			sender,
			driver: std::sync::Mutex::new( DriverState::Idle( Box::pin( async move {
				drop( receiver );
				AsyncRuntimeError::DriverStopped
			}))),
			interface_remaps: std::collections::HashMap::new(),
			export_asyncness: std::collections::HashMap::new(),
			session_locks: std::sync::Arc::new( Vec::new() ),
		}),
	)};
	let function = Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources );
	let result = futures::executor::block_on( plugin.dispatch_async( "test", "root", "call", &function, &[] ));
	let Err( DispatchError::RuntimeException( error )) = result else {
		panic!( "dispatch without a session did not return a runtime exception" );
	};
	assert_eq!( error.downcast_ref(), Some( &AsyncRuntimeError::SessionUnavailable ));

	let result = futures::executor::block_on( crate::dispatch_session::run( Vec::new(), async move {
		plugin.dispatch_async( "test", "root", "call", &function, &[] ).await
	}));
	let Err( DispatchError::RuntimeException( error )) = result else {
		panic!( "dropped response did not return a runtime exception" );
	};
	assert_eq!( error.downcast_ref(), Some( &AsyncRuntimeError::MissingResponse ));
}

#[test]
fn attached_driver_records_its_exact_terminal_error() {
	let ( sender, _receiver ) = futures::channel::mpsc::unbounded();
	let inner = std::sync::Arc::new( PluginInstanceAsyncInner {
		sender,
		driver: std::sync::Mutex::new( DriverState::Attached ),
		interface_remaps: std::collections::HashMap::new(),
		export_asyncness: std::collections::HashMap::new(),
		session_locks: std::sync::Arc::new( Vec::new() ),
	});
	let expected = AsyncRuntimeError::StoreFailed( "expected".to_string() );
	let mut driver = Box::pin( AttachedDriver {
		inner: std::sync::Arc::clone( &inner ),
		future: Some( Box::pin( futures::future::ready( expected.clone() ))),
	});
	let waker = futures::task::noop_waker();
	let mut context = std::task::Context::from_waker( &waker );
	assert!( driver.as_mut().poll( &mut context ).is_ready() );
	assert!( driver.as_mut().poll( &mut context ).is_ready() );
	let state = inner.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
	assert!( matches!( &*state, DriverState::Failed( error ) if error == &expected ));
}

#[test]
fn store_failure_reaches_pending_calls_as_the_exact_runtime_error() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let PluginState { store, instance, .. } = test_state_without_concurrency()?;
		let plugin = PluginInstanceAsync::new(
			store,
			instance,
			std::collections::HashMap::new(),
			std::collections::HashMap::new(),
			Vec::new(),
			None,
			None,
		);
		let expected = AsyncRuntimeError::StoreFailed(
			"cannot use `run_concurrent` when Config::concurrency_support disabled".to_string(),
		);
		let result = crate::dispatch_session::run( Vec::new(), async move {
			plugin.dispatch_async(
				"test:single-plugin",
				"root",
				"get-value",
				&Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
				&[],
			).await
		}).await;
		let Err( DispatchError::RuntimeException( error )) = result else {
			panic!( "pending call did not receive a runtime exception" );
		};
		assert_eq!( error.downcast_ref(), Some( &expected ));
		Ok(())
	})
}

#[test]
fn driver_finishes_admitted_calls_before_reporting_channel_shutdown() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut state = test_state().await?;
		let ( sender, receiver ) = futures::channel::mpsc::unbounded();
		let ( response, result ) = futures::channel::oneshot::channel();
		assert!( sender.unbounded_send( AsyncRequest {
			caller: 0,
			package_name: "test:single-async".to_string(),
			interface_name: "root".to_string(),
			function_name: "get-value".to_string(),
			function: Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
			data: Vec::new(),
			response,
		}).is_ok() );
		drop( sender );
		assert_eq!( state.run_requests( receiver ).await, AsyncRuntimeError::DriverStopped );
		assert!( matches!( result.await, Ok( Ok( Val::U32( 42 )))));
		Ok(())
	})
}

#[test]
fn concurrent_driver_reports_exact_export_lookup_errors() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut state = test_lookup_state().await?;
		let ( sender, receiver ) = futures::channel::mpsc::unbounded();
		let ( missing_interface_response, missing_interface ) = futures::channel::oneshot::channel();
		let mut missing_interface_request = request( 0, "get-value", missing_interface_response );
		missing_interface_request.package_name = "missing".to_string();
		missing_interface_request.interface_name = "root".to_string();
		assert!( sender.unbounded_send( missing_interface_request ).is_ok() );
		let ( missing_function_response, missing_function ) = futures::channel::oneshot::channel();
		let mut missing_function_request = request( 0, "missing", missing_function_response );
		missing_function_request.package_name = "test:dispatch-error".to_string();
		missing_function_request.interface_name = "root".to_string();
		assert!( sender.unbounded_send( missing_function_request ).is_ok() );
		let ( non_function_response, non_function ) = futures::channel::oneshot::channel();
		let mut non_function_request = request( 0, "not-a-function", non_function_response );
		non_function_request.package_name = "test:dispatch-error".to_string();
		non_function_request.interface_name = "root".to_string();
		assert!( sender.unbounded_send( non_function_request ).is_ok() );
		drop( sender );
		assert_eq!( state.run_requests( receiver ).await, AsyncRuntimeError::DriverStopped );
		assert!( matches!(
			missing_interface.await,
			Ok( Err( DispatchError::InvalidInterfacePath( path ))) if path == "missing/root"
		));
		assert!( matches!(
			missing_function.await,
			Ok( Err( DispatchError::InvalidFunction( name ))) if name == "test:dispatch-error/root:missing"
		));
		assert!( matches!(
			non_function.await,
			Ok( Err( DispatchError::InvalidFunction( name ))) if name == "test:dispatch-error/root:not-a-function"
		));
		Ok(())
	})
}

#[test]
fn limited_driver_reports_exact_channel_shutdown() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut state = test_state().await?;
		state.fuel_limiter = Some( Box::new(| _, _, _, _ | 1 ));
		let ( sender, receiver ) = futures::channel::mpsc::unbounded();
		drop( sender );
		assert_eq!( state.run_requests( receiver ).await, AsyncRuntimeError::RequestChannelClosed );
		Ok(())
	})
}

async fn test_state() -> Result<PluginState<Context>, Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let component = Component::from_file(
		&engine,
		concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatching/single_plugin_async/plugins/plugin/root.wat" ),
	)?;
	let linker = Linker::<Context>::new( &engine );
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let instance = linker.instantiate_async( &mut store, &component ).await?;
	Ok( PluginState {
		store,
		instance,
		interface_remaps: std::collections::HashMap::new(),
		fuel_limiter: None,
		epoch_limiter: None,
	})
}

async fn test_lookup_state() -> Result<PluginState<Context>, Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let component = Component::from_file(
		&engine,
		concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatch_error/invalid_function/plugins/test-plugin/root.wat" ),
	)?;
	let linker = Linker::<Context>::new( &engine );
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let instance = linker.instantiate_async( &mut store, &component ).await?;
	Ok( PluginState {
		store,
		instance,
		interface_remaps: std::collections::HashMap::new(),
		fuel_limiter: None,
		epoch_limiter: None,
	})
}

fn test_state_without_concurrency() -> Result<PluginState<Context>, wasmtime::Error> {
	let mut config = Config::new();
	config.concurrency_support( false );
	let engine = Engine::new( &config )?;
	let component = Component::from_file(
		&engine,
		concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatching/single_plugin_expect_primitive/plugins/get-value/root.wat" ),
	)?;
	let linker = Linker::<Context>::new( &engine );
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let instance = linker.instantiate( &mut store, &component )?;
	Ok( PluginState {
		store,
		instance,
		interface_remaps: std::collections::HashMap::new(),
		fuel_limiter: None,
		epoch_limiter: None,
	})
}

fn request(
	caller: u64,
	name: &str,
	response: futures::channel::oneshot::Sender<Result<Val, DispatchError>>,
) -> AsyncRequest {
	AsyncRequest {
		caller,
		package_name: String::new(),
		interface_name: String::new(),
		function_name: name.to_string(),
		function: Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
		data: Vec::new(),
		response,
	}
}

#[test]
fn accepts_nested_component_values() -> Result<(), DispatchError> {
	let value = Val::Record( vec![
		( "list".to_string(), Val::List( vec![ Val::U32( 1 ) ])),
		( "tuple".to_string(), Val::Tuple( vec![ Val::U32( 2 ) ])),
		( "map".to_string(), Val::Map( vec![( Val::String( "key".to_string() ), Val::U32( 3 ))])),
		( "variant".to_string(), Val::Variant( "some".to_string(), Some( Box::new( Val::U32( 4 ))))),
		( "option".to_string(), Val::Option( Some( Box::new( Val::U32( 5 ))))),
		( "ok".to_string(), Val::Result( Ok( Some( Box::new( Val::U32( 6 )))))),
		( "err".to_string(), Val::Result( Err( Some( Box::new( Val::U32( 7 )))))),
	]);
	ensure_supported_value( &value )
}

#[test]
fn rejects_future_and_stream_values() -> Result<(), Box<dyn std::error::Error>> {
	let mut config = Config::new();
	config.concurrency_support( true );
	let engine = Engine::new( &config )?;
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let future = FutureReader::new( &mut store, async { Ok::<_, wasmtime::Error>( 1_u32 )})?
		.try_into_future_any( &mut store )?;
	let stream = StreamReader::new( &mut store, vec![ 1_u32 ])?
		.try_into_stream_any( &mut store )?;

	assert!( matches!(
		ensure_supported_value( &Val::Future( future )),
		Err( DispatchError::UnsupportedType( name )) if name == "future"
	));
	assert!( matches!(
		ensure_supported_value( &Val::Stream( stream )),
		Err( DispatchError::UnsupportedType( name )) if name == "stream"
	));
	Ok(())
}

#[test]
fn rejects_error_context_values() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut config = Config::new();
		config.wasm_component_model_async( true );
		config.wasm_component_model_error_context( true );
		let engine = Engine::new( &config )?;
		let component = Component::from_file(
			&engine,
			concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/plugin_instance/error_context.wat" ),
		)?;
		let linker = Linker::<Context>::new( &engine );
		let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
		let instance = linker.instantiate_async( &mut store, &component ).await?;
		let function = instance.get_func( &mut store, "make-error-context" )
			.ok_or( "missing make-error-context export" )?;
		let mut results = [ Val::Bool( false ) ];
		function.call_async( &mut store, &[], &mut results ).await?;
		assert!( matches!(
			ensure_supported_value( &results[0] ),
			Err( DispatchError::UnsupportedType( name )) if name == "error-context"
		));
		Ok(())
	})
}
